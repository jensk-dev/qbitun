use reqwest::Client;
use serde_json::json;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from a .env file if present
    dotenv::dotenv().ok();

    // Read configuration from environment variables or use default values
    let qbittorrent_url = env::var("QBITTORRENT_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let qbittorrent_username = env::var("QBITTORRENT_USERNAME").unwrap_or_else(|_| "admin".to_string());
    let qbittorrent_password = env::var("QBITTORRENT_PASSWORD").unwrap_or_else(|_| "adminadmin".to_string());

    let gluetun_url = env::var("GLUETUN_URL").unwrap_or_else(|_| "http://localhost:8000/forwarded_port".to_string());

    // Get the port from Gluetun
    let port = get_gluetun_port(&gluetun_url).await?;

    // Login to qBittorrent
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()?;

    login_qbittorrent(&client, &qbittorrent_url, &qbittorrent_username, &qbittorrent_password).await?;

    // Set the port in qBittorrent
    set_qbittorrent_port(&client, &qbittorrent_url, port).await?;

    println!("Configured qBittorrent port to: {}", port);

    Ok(())
}

// Function to get the forwarded port from Gluetun
async fn get_gluetun_port(gluetun_url: &str) -> Result<u16, Box<dyn std::error::Error>> {
    let response = reqwest::get(gluetun_url).await?;
    let text = response.text().await?;
    let port: u16 = text.trim().parse()?;
    Ok(port)
}

// Function to login to qBittorrent's Web API
async fn login_qbittorrent(
    client: &Client,
    qbittorrent_url: &str,
    username: &str,
    password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let login_url = format!("{}/api/v2/auth/login", qbittorrent_url);
    let params = [("username", username), ("password", password)];

    let response = client.post(&login_url).form(&params).send().await?;

    let text = response.text().await?;

    if text != "Ok." {
        return Err("Failed to authenticate to qBittorrent. Please check your credentials and URL.".into());
    }

    Ok(())
}

// Function to set the listening port in qBittorrent's preferences
async fn set_qbittorrent_port(
    client: &Client,
    qbittorrent_url: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let set_prefs_url = format!("{}/api/v2/app/setPreferences", qbittorrent_url);
    let prefs = json!({ "listen_port": port });

    let params = [("json", prefs.to_string())];

    let response = client.post(&set_prefs_url).form(&params).send().await?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err("An error occurred while setting the port.".into())
    }
}
