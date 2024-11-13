use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use std::env;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, span, Level};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    #[cfg(debug_assertions)]
    dotenv::dotenv().ok();

    let qbittorrent_url = get_env_var("QBITTORRENT_URL")?;
    let qbittorrent_username = get_env_var("QBITTORRENT_USERNAME")?;
    let qbittorrent_password = get_secret_from_env("QBITTORRENT_PASSWORD")?;
    let gluetun_url = get_env_var("GLUETUN_URL")?;
    let gluetun_api_key = get_secret_from_env("GLUETUN_API_KEY")?;
    let interval_seconds: u64 = match get_env_var("INTERVAL_SECONDS")?.parse() {
        Ok(seconds) => seconds,
        Err(e) => {
            let error_message = format!("Failed to parse INTERVAL_SECONDS: {}", e);
            error!("{}", error_message);
            return Err(error_message.into());
        }
    };

    let app_span = span!(Level::INFO, "qbitun");
    let _enter = app_span.enter();

    info!("Starting qBittorrent and Gluetun port synchronization");

    let client = reqwest::Client::builder().cookie_store(true).build()?;

    loop {
        sync_ports(&client, &qbittorrent_url, &qbittorrent_username, &qbittorrent_password, &gluetun_url, &gluetun_api_key).await;
        info!("Waiting for {} seconds before the next synchronization", interval_seconds);
        sleep(Duration::from_secs(interval_seconds)).await;
    }
}

/// Retrieves a secret environment variable and removes it from the environment.
///
/// # Parameters
///
/// * `var_name` - The name of the environment variable to retrieve.
///
/// # Returns
///
/// Returns the secret as a `SecretString` on success, or an error on failure.
#[instrument]
fn get_secret_from_env(var_name: &str) -> Result<SecretString, Box<dyn std::error::Error>> {
    match env::var(var_name) {
        Ok(secret) => {
            env::remove_var(var_name);
            Ok(SecretString::from(secret))
        }
        Err(_) => {
            let error_message = format!("{} environment variable is not set", var_name);
            error!("{}", error_message);
            Err(error_message.into())
        }
    }
}

/// Retrieves an environment variable.
///
/// # Parameters
///
/// * `var_name` - The name of the environment variable to retrieve.
///
/// # Returns
///
/// Returns the value of the environment variable as a `String` on success, or an error on failure.
#[instrument]
fn get_env_var(var_name: &str) -> Result<String, Box<dyn std::error::Error>> {
    match env::var(var_name) {
        Ok(value) => Ok(value),
        Err(_) => {
            let error_message = format!("{} environment variable is not set", var_name);
            error!("{}", error_message);
            Err(error_message.into())
        }
    }
}

/// Synchronizes the port configuration between qBittorrent and Gluetun.
///
/// This function retrieves the forwarded port from Gluetun, logs in to qBittorrent,
/// and updates the qBittorrent listening port if necessary.
///
/// # Parameters
///
/// * `client` - The HTTP client to use for making requests.
/// * `qbittorrent_url` - The URL of the qBittorrent Web API.
/// * `qbittorrent_username` - The username for qBittorrent authentication.
/// * `qbittorrent_password` - The password for qBittorrent authentication.
/// * `gluetun_url` - The URL to retrieve the forwarded port from Gluetun.
/// * `gluetun_api_key` - The API key for Gluetun authentication.
#[instrument(skip(client, qbittorrent_password, gluetun_api_key))]
async fn sync_ports(
    client: &Client,
    qbittorrent_url: &str,
    qbittorrent_username: &str,
    qbittorrent_password: &SecretString,
    gluetun_url: &str,
    gluetun_api_key: &SecretString,
) {
    // Get the port from Gluetun
    let port = match get_gluetun_port(gluetun_url, gluetun_api_key).await {
        Ok(port) => {
            info!(port, "Retrieved forwarded port from Gluetun");
            port
        }
        Err(e) => {
            error!(error = %e, "Failed to retrieve port from Gluetun");
            return;
        }
    };

    // Login to qBittorrent
    if let Err(e) = login_qbittorrent(
        client,
        qbittorrent_url,
        qbittorrent_username,
        qbittorrent_password,
    )
    .await
    {
        error!(error = %e, "Failed to authenticate with qBittorrent");
        return;
    } else {
        debug!("Authenticated with qBittorrent");
    }

    // Get the current port from qBittorrent
    let current_port = match get_qbittorrent_port(client, qbittorrent_url).await {
        Ok(port) => port,
        Err(e) => {
            error!(error = %e, "Failed to get current qBittorrent port");
            return;
        }
    };

    // Update the port if it's different
    if current_port != port {
        if let Err(e) = set_qbittorrent_port(client, qbittorrent_url, port).await {
            error!(error = %e, "Failed to set qBittorrent port");
        } else {
            info!(port, "Configured qBittorrent listening port");
        }
    } else {
        info!(
            port,
            "qBittorrent is already configured with the correct port"
        );
    }
}

#[derive(Debug, serde::Deserialize)]
struct GluetunPort {
    port: u16,
}

/// Retrieves the forwarded port from Gluetun.
///
/// # Parameters
///
/// * `gluetun_url` - The URL to retrieve the forwarded port from.
/// * `gluetun_api_key` - The API key for Gluetun authentication.
///
/// # Returns
///
/// Returns the forwarded port as a `u16` on success, or an error on failure.
#[instrument(skip(gluetun_api_key))]
async fn get_gluetun_port(
    gluetun_url: &str,
    gluetun_api_key: &SecretString,
) -> Result<u16, Box<dyn std::error::Error>> {
    let gluetun_url = format!("{}/v1/openvpn/portforwarded", gluetun_url);
    debug!(gluetun_url, "Sending request to Gluetun");
    let response = reqwest::Client::new()
        .get(gluetun_url)
        .header("X-API-Key", gluetun_api_key.expose_secret())
        .send()
        .await?;
    let gluetun_port: GluetunPort = response.json().await?;
    Ok(gluetun_port.port)
}

/// Logs in to qBittorrent's Web API.
///
/// # Parameters
///
/// * `client` - The HTTP client to use for making requests.
/// * `qbittorrent_url` - The URL of the qBittorrent Web API.
/// * `username` - The username for qBittorrent authentication.
/// * `password` - The password for qBittorrent authentication.
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error on failure.
#[instrument(skip(client, password))]
async fn login_qbittorrent(
    client: &Client,
    qbittorrent_url: &str,
    username: &str,
    password: &SecretString,
) -> Result<(), Box<dyn std::error::Error>> {
    let login_url = format!("{}/api/v2/auth/login", qbittorrent_url);
    let params = [
        ("username", username),
        ("password", password.expose_secret()),
    ];

    debug!("Attempting to authenticate with qBittorrent");
    let response = client
        .post(&login_url)
        .header("Referer", qbittorrent_url)
        .form(&params)
        .send()
        .await?;

    if response.status().is_success() {
        debug!("Authentication successful with qBittorrent");
        Ok(())
    } else if response.status().as_u16() == 403 {
        error!("Authentication failed with qBittorrent: User's IP is banned for too many failed login attempts");
        Err("Authentication failed with qBittorrent: User's IP is banned for too many failed login attempts".into())
    } else {
        error!("Authentication failed with qBittorrent");
        Err("Failed to authenticate to qBittorrent. Please check your credentials and URL.".into())
    }
}

/// Retrieves the current listening port from qBittorrent.
///
/// # Parameters
///
/// * `client` - The HTTP client to use for making requests.
/// * `qbittorrent_url` - The URL of the qBittorrent Web API.
///
/// # Returns
///
/// Returns the current listening port as a `u16` on success, or an error on failure.
#[instrument(skip(client))]
async fn get_qbittorrent_port(
    client: &Client,
    qbittorrent_url: &str,
) -> Result<u16, Box<dyn std::error::Error>> {
    let prefs_url = format!("{}/api/v2/app/preferences", qbittorrent_url);

    debug!("Retrieving current qBittorrent preferences");
    let response = client.get(&prefs_url).send().await?;

    if response.status().is_success() {
        let prefs: serde_json::Value = response.json().await?;
        if let Some(port) = prefs.get("listen_port").and_then(|p| p.as_u64()) {
            Ok(port as u16)
        } else {
            Err("listen_port not found in preferences".into())
        }
    } else {
        Err("Failed to retrieve qBittorrent preferences".into())
    }
}

/// Sets the listening port for qBittorrent.
///
/// # Parameters
///
/// * `client` - The HTTP client to use for making requests.
/// * `qbittorrent_url` - The URL of the qBittorrent Web API.
/// * `port` - The port to set as the listening port.
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error on failure.
#[instrument(skip(client))]
async fn set_qbittorrent_port(
    client: &Client,
    qbittorrent_url: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let set_prefs_url = format!("{}/api/v2/app/setPreferences", qbittorrent_url);
    let prefs = json!({ "listen_port": port });

    let params = [("json", prefs.to_string())];

    debug!(port, "Setting qBittorrent listening port");
    let response = client.post(&set_prefs_url).form(&params).send().await?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err("An error occurred while setting the port.".into())
    }
}
