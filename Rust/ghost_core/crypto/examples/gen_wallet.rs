use crypto::wallet::Wallet;

fn main() {
    let wallet = Wallet::generate();
    println!("Address:     {}", wallet.address);
    println!("Public key:  {}", wallet.public_key_hex());
    println!("Private key: {}", wallet.private_key_hex());
    println!();
    println!("Save your private key! You need it to send transactions.");
}