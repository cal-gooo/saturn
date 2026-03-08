use std::io::{self, Read};

use a2a_commerce_protocol::security::signing::sign_value;
use serde_json::Value;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let secret_key = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("A2AC_SIGNING_SECRET").ok())
        .unwrap_or_else(|| {
            "1111111111111111111111111111111111111111111111111111111111111111".into()
        });

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut value: Value = serde_json::from_str(&input)?;
    sign_value(&mut value, &secret_key)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
