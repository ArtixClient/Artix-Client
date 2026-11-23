//! Official Microsoft -> Xbox Live -> Minecraft Services sign-in (device code flow).
//! The launcher talks ONLY to Microsoft / Xbox / Mojang endpoints; there is no backend and nothing is sent to the launcher's author.
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::net::{self, R};

/// Azure app (client) ID. Register your own free app and set it at build time:  ARTIX_CLIENT_ID=<id> cargo build --release
/// See README ("Microsoft sign-in") for the one-time setup.
pub const CLIENT_ID: &str = match option_env!("ARTIX_CLIENT_ID") {
    Some(s) => s,
    None => "YOUR_AZURE_CLIENT_ID",
};

pub fn configured() -> bool {
    CLIENT_ID.len() > 20 && !CLIENT_ID.contains("YOUR_")
}

const DEVICE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";

#[derive(Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub uri: String,
    device_code: String,
    interval: u64,
    expires_in: u64,
}

pub struct McSession {
    pub token: String,
    pub expires_at: u64,
    pub uuid: String,
    pub name: String,
    pub owns: bool,
    pub profile: Value,
}

pub fn start_device() -> R<DeviceCode> {
    let (st, v) = net::post_form(DEVICE_URL, &[("client_id", CLIENT_ID), ("scope", SCOPE)])?;
    if st != 200 {
        let d = v["error_description"].as_str().unwrap_or("Microsoft refused the request");
        return Err(format!("{d} (check your Azure client ID)"));
    }
    Ok(DeviceCode {
        user_code: v["user_code"].as_str().unwrap_or("").into(),
        uri: v["verification_uri"].as_str().unwrap_or("https://www.microsoft.com/link").into(),
        device_code: v["device_code"].as_str().unwrap_or("").into(),
        interval: v["interval"].as_u64().unwrap_or(5),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
    })
}

/// Blocks until the user finishes signing in (or cancels / times out). Returns (ms_access_token, refresh_token).
pub fn poll_device(dc: &DeviceCode, cancel: &AtomicBool) -> R<(String, String)> {
    let mut waited = 0u64;
    let mut interval = dc.interval.max(1);
    while waited < dc.expires_in {
        for _ in 0..interval {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        waited += interval;
        let (_, v) = net::post_form(
            TOKEN_URL,
            &[("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("client_id", CLIENT_ID), ("device_code", &dc.device_code)],
        )?;
        if let Some(t) = v["access_token"].as_str() {
            return Ok((t.into(), v["refresh_token"].as_str().unwrap_or("").into()));
        }
        match v["error"].as_str().unwrap_or("") {
            "authorization_pending" => {}
            "slow_down" => interval += 5,
            "expired_token" | "authorization_declined" | "bad_verification_code" => return Err("Sign-in was cancelled or expired".into()),
            other => return Err(format!("Microsoft error: {other}")),
        }
    }
    Err("Sign-in timed out".into())
}

pub fn refresh_ms(refresh: &str) -> R<(String, String)> {
    let (st, v) = net::post_form(TOKEN_URL, &[("grant_type", "refresh_token"), ("client_id", CLIENT_ID), ("refresh_token", refresh), ("scope", SCOPE)])?;
    match v["access_token"].as_str() {
        Some(t) if st == 200 => Ok((t.into(), v["refresh_token"].as_str().unwrap_or(refresh).into())),
        _ => Err("Your Microsoft sign-in expired. Please sign in again.".into()),
    }
}

fn xsts_error(code: u64) -> &'static str {
    match code {
        2148916233 => "This Microsoft account has no Xbox profile. Sign in once at xbox.com to create one.",
        2148916235 => "Xbox Live isn't available in your country.",
        2148916236 | 2148916237 => "This account needs adult verification on Xbox.",
        2148916238 => "This is a child account. Add it to a Microsoft family group first.",
        _ => "Xbox authorization failed.",
    }
}

/// Microsoft access token -> Xbox Live -> XSTS -> Minecraft token -> ownership + profile check.
pub fn login_with_ms(ms_access: &str) -> R<McSession> {
    let (st, xbl) = net::post_json(
        "https://user.auth.xboxlive.com/user/authenticate",
        &json!({"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={ms_access}")},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"}),
        None,
    )?;
    if st != 200 {
        return Err("Xbox Live sign-in failed.".into());
    }
    let xbl_token = xbl["Token"].as_str().ok_or("Xbox Live returned no token")?.to_string();

    let (st, xsts) = net::post_json(
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        &json!({"Properties":{"SandboxId":"RETAIL","UserTokens":[xbl_token]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"}),
        None,
    )?;
    if st != 200 {
        return Err(xsts_error(xsts["XErr"].as_u64().unwrap_or(0)).into());
    }
    let xsts_token = xsts["Token"].as_str().ok_or("XSTS returned no token")?;
    let uhs = xsts["DisplayClaims"]["xui"][0]["uhs"].as_str().ok_or("XSTS returned no user hash")?;

    let (st, mc) = net::post_json(
        "https://api.minecraftservices.com/authentication/login_with_xbox",
        &json!({"identityToken": format!("XBL3.0 x={uhs};{xsts_token}")}),
        None,
    )?;
    if st != 200 {
        let msg = if st == 403 { "Minecraft services rejected this launcher's app registration (see README: Microsoft sign-in)." } else { "Minecraft sign-in failed." };
        return Err(msg.into());
    }
    let token = mc["access_token"].as_str().ok_or("No Minecraft token")?.to_string();
    let expires_at = crate::sys::unix_now() + mc["expires_in"].as_u64().unwrap_or(86400).saturating_sub(120);

    // Official ownership / profile check. 200 = owns Java Edition and has a profile, 404 = no profile.
    let (st, body) = net::get_bytes_auth("https://api.minecraftservices.com/minecraft/profile", &token)?;
    let profile: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if st == 200 && profile["id"].is_string() {
        Ok(McSession { token, expires_at, uuid: profile["id"].as_str().unwrap().into(), name: profile["name"].as_str().unwrap_or("Player").into(), owns: true, profile })
    } else {
        Err("This Microsoft account doesn't own Minecraft: Java Edition (no game profile found).".into())
    }
}

/// Refreshes the account's Minecraft token if it expired. Returns the new profile when a refresh happened.
pub fn ensure(a: &mut crate::store::Account) -> R<Option<Value>> {
    if crate::sys::unix_now() < a.mc_expires && !a.mc_token.is_empty() {
        return Ok(None);
    }
    let (ms, rt) = refresh_ms(&a.refresh)?;
    let s = login_with_ms(&ms)?;
    a.refresh = rt;
    a.mc_token = s.token;
    a.mc_expires = s.expires_at;
    a.uuid = s.uuid;
    a.name = s.name;
    a.owns = true;
    Ok(Some(s.profile))
}
