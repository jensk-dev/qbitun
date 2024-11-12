use reqwest::Client;
use serde_json::json;
use std::env;
use tracing::{debug, error, info, instrument, span, warn, Level};
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
    let qbittorrent_url = env::var("QBITTORRENT_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let qbittorrent_username = env::var("QBITTORRENT_USERNAME").unwrap_or_else(|_| "admin".to_string());
    let qbittorrent_password = env::var("QBITTORRENT_PASSWORD").unwrap_or_else(|_| "adminadmin".to_string());

    let gluetun_url = env::var("GLUETUN_URL").unwrap_or_else(|_| "http://localhost:8000/forwarded_port".to_string());

    // Create a root span for the application
    let app_span = span!(Level::INFO, "qbitun");
    let _enter = app_span.enter();

    info!("Starting qBittorrent and Gluetun port synchronization");

    // Get the port from Gluetun
    let port = match get_gluetun_port(&gluetun_url).await {
        Ok(port) => {
            info!(port, "Retrieved forwarded port from Gluetun");
            port
        }
        Err(e) => {
            error!(error = %e, "Failed to retrieve port from Gluetun");
            return Err(e);
        }
    };

    // Login to qBittorrent
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()?;

    if let Err(e) = login_qbittorrent(&client, &qbittorrent_url, &qbittorrent_username, &qbittorrent_password).await {
        error!(error = %e, "Failed to authenticate with qBittorrent");
        return Err(e);
    } else {
        info!("Authenticated with qBittorrent");
    }

    // Set the port in qBittorrent
    if let Err(e) = set_qbittorrent_port(&client, &qbittorrent_url, port).await {
        error!(error = %e, "Failed to set qBittorrent port");
        return Err(e);
    } else {
        info!(port, "Configured qBittorrent listening port");
    }

    Ok(())
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
    password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let login_url = format!("{}/api/v2/auth/login", qbittorrent_url);
    let params = [("username", username), ("password", password)];

    debug!("Attempting to authenticate with qBittorrent");
    let response = client.post(&login_url).form(&params).send().await?;

    let text = response.text().await?;

    if text != "Ok." {
        error!("Authentication failed with qBittorrent");
        return Err("Failed to authenticate to qBittorrent. Please check your credentials and URL.".into());
    }

    Ok(())
}

// Function to set the listening port in qBittorrent's preferences
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
        error!("Failed to set qBittorrent listening port");
        Err("An error occurred while setting the port.".into())
    }
}
