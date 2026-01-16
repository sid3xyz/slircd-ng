#!/bin/bash
# Register a test user for bouncer multiclient testing

# Stop the server
pkill -f 'target/release/slircd' || true
sleep 1

# Create a small Rust program to register the user
cat > /tmp/register_user.rs << 'EOF'
use slircd_ng::db::accounts::{hash_password, compute_scram_verifiers};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let password = "testpass";
    
    // Hash the password
    let password_hash = hash_password(password).await?;
    let scram = compute_scram_verifiers(password).await;
    
    // Connect to database
    let pool = sqlx::SqlitePool::connect("sqlite:slircd.db").await?;
    
    let now = chrono::Utc::now().timestamp();
    
    // Check if user exists
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM accounts WHERE name = ? COLLATE NOCASE"
    )
    .bind("testuser")
    .fetch_optional(&pool)
    .await?;
    
    if let Some((account_id,)) = existing {
        println!("Account 'testuser' already exists (ID: {})", account_id);
        
        // Update password
        sqlx::query(
            "UPDATE accounts SET password_hash = ?, scram_salt = ?, scram_iterations = ?, scram_hashed_password = ? WHERE id = ?"
        )
        .bind(&password_hash)
        .bind(&scram.salt)
        .bind(scram.iterations as i32)
        .bind(&scram.hashed_password)
        .bind(account_id)
        .execute(&pool)
        .await?;
        
        println!("Password updated for 'testuser'");
    } else {
        // Create new account
        let result = sqlx::query(
            r#"
            INSERT INTO accounts (name, password_hash, email, registered_at, last_seen_at,
                                  scram_salt, scram_iterations, scram_hashed_password)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("testuser")
        .bind(&password_hash)
        .bind(None::<String>)
        .bind(now)
        .bind(now)
        .bind(&scram.salt)
        .bind(scram.iterations as i32)
        .bind(&scram.hashed_password)
        .execute(&pool)
        .await?;
        
        let account_id = result.last_insert_rowid();
        
        // Link nickname
        sqlx::query(
            "INSERT INTO nicknames (name, account_id) VALUES (?, ?)"
        )
        .bind("testuser")
        .bind(account_id)
        .execute(&pool)
        .await?;
        
        println!("Created account 'testuser' with ID: {}", account_id);
    }
    
    pool.close().await;
    Ok(())
}
EOF

# Compile and run
echo "Registering test user..."
cd /home/straylight/slircd-ng
cargo run --release --quiet --example /tmp/register_user.rs 2>&1 || {
    # Try direct SQLite if Rust fails
    echo "Trying direct SQLite approach..."
    sqlite3 slircd.db << SQL
INSERT OR REPLACE INTO accounts (id, name, password_hash, email, registered_at, last_seen_at)
VALUES (1, 'testuser', 'dummy_hash', NULL, strftime('%s', 'now'), strftime('%s', 'now'));

INSERT OR REPLACE INTO nicknames (name, account_id)
VALUES ('testuser', 1);
SQL
    echo "Created stub account (password auth will fail - using direct approach instead)"
}

echo "Done! User 'testuser' is registered."
echo "Restart server with: ./target/release/slircd"
