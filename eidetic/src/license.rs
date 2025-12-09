use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result, anyhow};

// Freemius Configuration
// TODO: Replace with your actual Product ID and Keys
const PRODUCT_ID: &str = "22217";
const PUBLIC_KEY: &str = "pk_449d4c5954dccbb796d8b2648e1aa";

// For activation, we might not need the Secret Key if using public-facing activation 
// that is properly scoped, but usually client-side activation uses the public key 
// or a specific user token. 
// Freemius API typically requires generating a signature for secure requests, 
// but for simple license activation via their API, we follow their specific flow.
// Note: Activating via API often requires Secret Key if not done via their SDK/Checkout.
// If purely client-side without secret key, we rely on the user finding their key from email.

// However, the user request says: 
// "App sends request to Freemius API: POST /v1/products/{id}/licenses/activate.json"
// This endpoint usually requires valid authentication.

#[derive(Serialize, Deserialize, Debug)]
pub struct LicenseData {
    pub id: u64,
    pub public_key: String,
    pub secret_key: String,
    pub is_active: bool,
    pub expiration: Option<String>,
    // Add other fields as necessary from Freemius response
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalLicense {
    pub key: String,
    pub id: u64,
}

pub fn get_license_file_path() -> Result<PathBuf> {
    let mut path = dirs::home_dir().context("Could not find home directory")?;
    path.push(".eidetic");
    if !path.exists() {
        fs::create_dir_all(&path)?;
    }
    path.push("license.json");
    Ok(path)
}

pub fn load_license() -> Result<LocalLicense> {
    let path = get_license_file_path()?;
    if !path.exists() {
        return Err(anyhow!("No license file found"));
    }
    let content = fs::read_to_string(path)?;
    let license: LocalLicense = serde_json::from_str(&content)?;
    Ok(license)
}

pub fn save_license(license: &LocalLicense) -> Result<()> {
    let path = get_license_file_path()?;
    let content = serde_json::to_string_pretty(license)?;
    fs::write(path, content)?;
    Ok(())
}

/// Activates a license key with Freemius.
/// 
/// Note: This is a simplified implementation. Real Freemius API calls 
/// often require signing requests with HmacSHA256 if using the Secret Key,
/// or might have specific headers.
/// 
/// Based on standard Freemius API docs for simpler integrations or if using a proxy:
/// We will try a direct hit to their API. If this fails due to auth (needs signing),
/// we might need to implement the signature generation or route through our Worker.
/// 
/// User said: "App sends request to Freemius API: POST /v1/products/{id}/licenses/activate.json"
pub fn activate_license(license_key: String) -> Result<LocalLicense> {
    let client = reqwest::blocking::Client::new();
    let url = format!("https://api.freemius.com/v1/products/{}/licenses/activate.json", PRODUCT_ID);

    // Payload for activation
    // Freemius often expects 'license_key' in the body
    let params = [("license_key", &license_key)];
    
    // Authorization is tricky here. Client-side apps usually can't hold the Secret Key securely.
    // If Freemius allows Public Key for activation context it's fine. 
    // Otherwise, we might need to route this through our backend worker?
    // User instruction implied direct app request. We will attempt standard request.
    
    // Note: In many Freemius implementations, you just check if the key exists and matches.
    // Actual "activation" (binding to a user/site) might require existing user context.
    // Let's assume for this "Product" type, we can validate the key.
    
    // ALTERNATIVE: GET /v1/products/{id}/licenses.json?filter=key&public_key=...
    // But that might return all licenses? No.
    
    // Let's implement the specific endpoint requested by user logic.
    let response = client.put(&url) // 'activate' is often a PUT or POST
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&serde_json::json!({
             "license_key": license_key
        }))?)
        // .basic_auth(PUBLIC_KEY, Some("secret?")) // Unsafe to put secret here
        .send();

    // IF the above is too complex/undocumented without specific auth headers (Date, Auth signature),
    // we might need to route this via our Worker or ask User for the specific Freemius setup.
    
    // FOR NOW: We will implement a "Check" logic which is safer and easier.
    // We check if the license key is valid by fetching it.
    // Since we don't have the full Freemius Auth implementation (HMAC signing) here,
    // and storing Secret Key in the binary is bad practice,
    // we strongly recommend using the Worker as a proxy for this if request signing is needed.
    
    // HOWEVER, to unblock the user, we'll create the structure and assume 
    // they might have a proxy or specific public endpoint enabled.
    
    // MOCK RESPONSE for initial development until keys are real
    // Remove this in production
    if license_key.starts_with("ED-") {
        let mock = LocalLicense {
            key: license_key,
            id: 12345,
        };
        save_license(&mock)?;
        return Ok(mock);
    }

    Err(anyhow!("Failed to activate license (Implementation requires valid API Keys)"))
}

/// Checks if a license is still active.
pub fn check_license_status() -> Result<bool> {
    let license = load_license()?;
    
    // Logic:
    // GET /v1/products/{id}/licenses/{license_id}.json
    // Check `is_active` and `expiration`
    
    // Again, requires API Auth (likely HmacSHA256).
    // For now, we return true if we have a saved license, assuming 'activate' did the heavy lifting.
    // In a real scenario, this function would make a network call.
    
    // Mock check:
    if !license.key.is_empty() {
        return Ok(true);
    }

    Ok(false)
}
