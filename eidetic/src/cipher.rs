// Simple XOR-Rotate Cipher for Vault Prototype
// In production, use AES-GCM (ring/aes-gcm crate).
// This is sufficient to prove the "Transparent Encryption" architecture.

const KEY: u8 = 0xAA; // Secret Key

pub fn encrypt(data: &[u8]) -> Vec<u8> {
    data.iter().enumerate().map(|(i, &b)| {
        let k = KEY.wrapping_add((i % 255) as u8);
        b.wrapping_add(k) ^ k // bitwise XOR
    }).collect()
}

pub fn decrypt(data: &[u8]) -> Vec<u8> {
    data.iter().enumerate().map(|(i, &b)| {
        let k = KEY.wrapping_add((i % 255) as u8);
        (b ^ k).wrapping_sub(k) // bitwise XOR
    }).collect()
}
