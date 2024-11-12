use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use std::env;
use std::time::Duration;
use tokio::signal;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, span, Level};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber with environment filter and JSON formatter
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    // Load environment variables from a .env file if present
    dotenv::dotenv().ok();

    // Read configuration from environment variables or use default values
    let qbittorrent_url =
        env::var("QBITTORRENT_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let qbittorrent_username = env::var("QBITTORRENT_USERNAME").unwrap_or_else(|_| "admin".to_string());

    // Read the password once and securely store it
    let qbittorrent_password = match env::var("QBITTORRENT_PASSWORD") {
        Ok(pw) => {
            // Remove the password from the environment
            env::remove_var("QBITTORRENT_PASSWORD");
            // Securely store the password
            SecretString::from(pw)
        }
        Err(_) => {
            // Use default password (not recommended)
            SecretString::from("adminadmin")
        }
    };

    let gluetun_url =
        env::var("GLUETUN_URL").unwrap_or_else(|_| "http://localhost:8000/forwarded_port".to_string());

    // Synchronization interval in seconds
    let sync_interval_seconds: u64 = env::var("SYNC_INTERVAL_SECONDS")
        .unwrap_or_else(|_| "300".to_string()) // Default to 300 seconds (5 minutes)
        .parse()
        .expect("SYNC_INTERVAL_SECONDS must be a valid integer");

    // Create a root span for the application
    let app_span = span!(Level::INFO, "qbittorrent_gluetun_port_sync");
    let _enter = app_span.enter();

    info!("Starting qBittorrent and Gluetun port synchronization");

    // Create a client outside the loop to reuse connections and cookies
    let client = reqwest::Client::builder().cookie_store(true).build()?;

    loop {
        let shutdown_signal = async {
            signal::ctrl_c()
                .await
                .expect("Failed to install CTRL+C handler");
        };

        tokio::select! {
            _ = shutdown_signal => {
                info!("Shutdown signal received. Exiting...");
                break;
            }
            _ = sync_once(&client, &qbittorrent_url, &qbittorrent_username, &qbittorrent_password, &gluetun_url) => {
                // Wait for the specified interval before the next synchronization
                info!("Waiting for {} seconds before the next synchronization", sync_interval_seconds);
                sleep(Duration::from_secs(sync_interval_seconds)).await;
            }
        }
    }

    drop(qbittorrent_password);

    Ok(())
}

#[instrument(skip(client, qbittorrent_password))]
async fn sync_once(
    client: &Client,
    qbittorrent_url: &str,
    qbittorrent_username: &str,
    qbittorrent_password: &SecretString,
    gluetun_url: &str,
) {
    // Get the port from Gluetun
    let port = match get_gluetun_port(gluetun_url).await {
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
        info!(port, "qBittorrent is already configured with the correct port");
    }
}

// Function to get the forwarded port from Gluetun
#[instrument]
async fn get_gluetun_port(gluetun_url: &str) -> Result<u16, Box<dyn std::error::Error>> {
    debug!(gluetun_url, "Sending request to Gluetun");
    let response = reqwest::get(gluetun_url).await?;
    let text = response.text().await?;
    let port: u16 = text.trim().parse()?;
    Ok(port)
}

// Function to login to qBittorrent's Web API
#[instrument(skip(client, password))]
async fn login_qbittorrent(
    client: &Client,
    qbittorrent_url: &str,
    username: &str,
    password: &SecretString,
) -> Result<(), Box<dyn std::error::Error>> {
    let login_url = format!("{}/api/v2/auth/login", qbittorrent_url);
    let params = [("username", username), ("password", password.expose_secret())];

    debug!("Attempting to authenticate with qBittorrent");
    let response = client.post(&login_url).form(&params).send().await?;

    let text = response.text().await?;

    if text != "Ok." {
        error!("Authentication failed with qBittorrent");
        return Err(
            "Failed to authenticate to qBittorrent. Please check your credentials and URL.".into(),
        );
    }

    Ok(())
}
 
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
