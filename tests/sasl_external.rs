mod common;

use common::{TestClient, TestServer};
use std::time::Duration;

#[tokio::test]
async fn test_sasl_external_with_client_cert() -> anyhow::Result<()> {
    let server = TestServer::spawn_tls(17669, 17670)
        .await
        .expect("Failed to spawn TLS test server");

    let mut registrar = server
        .connect_tls_with_client_cert("certuser")
        .await
        .expect("Failed to connect TLS client with cert");
    registrar.register().await?;

    registrar.send_raw("CAP LS 302\r\n").await?;
    wait_for_contains(&mut registrar, "EXTERNAL", "CAP LS with EXTERNAL").await?;

    registrar
        .send_raw("PRIVMSG NickServ :REGISTER extpass ext@example.com\r\n")
        .await?;
    wait_for_contains(
        &mut registrar,
        "identified",
        "NickServ REGISTER identification",
    )
    .await?;

    registrar.send_raw("PRIVMSG NickServ :CERT ADD\r\n").await?;
    wait_for_contains(
        &mut registrar,
        "Certificate fingerprint added",
        "NickServ CERT ADD",
    )
    .await?;

    registrar.quit(None).await?;
    drop(registrar);

    let mut sasl_client = server
        .connect_tls_with_client_cert("certauth")
        .await
        .expect("Failed to connect TLS client with cert");
    sasl_client.send_raw("CAP REQ :sasl\r\n").await?;
    wait_for_cap_ack(&mut sasl_client).await?;

    sasl_client.send_raw("AUTHENTICATE EXTERNAL\r\n").await?;
    wait_for_authenticate_plus(&mut sasl_client).await?;
    sasl_client.send_raw("AUTHENTICATE +\r\n").await?;
    wait_for_sasl_success(&mut sasl_client).await?;

    let mut no_cert_client = server
        .connect_tls_without_client_cert("nocert")
        .await
        .expect("Failed to connect TLS client without cert");
    no_cert_client.send_raw("CAP REQ :sasl\r\n").await?;
    wait_for_cap_ack(&mut no_cert_client).await?;

    no_cert_client.send_raw("AUTHENTICATE EXTERNAL\r\n").await?;
    wait_for_sasl_fail(&mut no_cert_client).await?;

    Ok(())
}

/// Test SASL EXTERNAL fails when cert is present but not registered to any account.
#[tokio::test]
async fn test_sasl_external_unregistered_cert_fails() -> anyhow::Result<()> {
    let server = TestServer::spawn_tls(17671, 17672)
        .await
        .expect("Failed to spawn TLS test server");

    // Connect with a client cert, but don't register the fingerprint
    let mut client = server
        .connect_tls_with_client_cert("unregistered")
        .await
        .expect("Failed to connect TLS client with cert");

    client.send_raw("CAP REQ :sasl\r\n").await?;
    wait_for_cap_ack(&mut client).await?;

    // Attempt EXTERNAL auth - should fail because cert not registered
    client.send_raw("AUTHENTICATE EXTERNAL\r\n").await?;
    wait_for_authenticate_plus(&mut client).await?;
    client.send_raw("AUTHENTICATE +\r\n").await?;
    wait_for_sasl_fail(&mut client).await?;

    Ok(())
}

async fn wait_for_contains(
    client: &mut TestClient,
    needle: &str,
    context: &str,
) -> anyhow::Result<()> {
    for _ in 0..20 {
        if let Ok(msg) = client.recv_timeout(Duration::from_secs(1)).await
            && msg.to_string().contains(needle)
        {
            return Ok(());
        }
    }
    anyhow::bail!("Timed out waiting for {context}")
}

async fn wait_for_cap_ack(client: &mut TestClient) -> anyhow::Result<()> {
    for _ in 0..20 {
        if let Ok(msg) = client.recv_timeout(Duration::from_secs(1)).await {
            let text = msg.to_string();
            if text.contains("CAP") && text.contains("ACK") {
                return Ok(());
            }
        }
    }
    anyhow::bail!("Timed out waiting for CAP ACK")
}

async fn wait_for_authenticate_plus(client: &mut TestClient) -> anyhow::Result<()> {
    for _ in 0..20 {
        if let Ok(msg) = client.recv_timeout(Duration::from_secs(1)).await
            && msg.to_string().contains("AUTHENTICATE +")
        {
            return Ok(());
        }
    }
    anyhow::bail!("Timed out waiting for AUTHENTICATE +")
}

async fn wait_for_sasl_success(client: &mut TestClient) -> anyhow::Result<()> {
    for _ in 0..20 {
        if let Ok(msg) = client.recv_timeout(Duration::from_secs(1)).await {
            let text = msg.to_string();
            if text.contains("903") || (text.contains("SASL") && text.contains("success")) {
                return Ok(());
            }
        }
    }
    anyhow::bail!("Timed out waiting for SASL success")
}

async fn wait_for_sasl_fail(client: &mut TestClient) -> anyhow::Result<()> {
    for _ in 0..20 {
        if let Ok(msg) = client.recv_timeout(Duration::from_secs(1)).await {
            let text = msg.to_string();
            if text.contains("904") || (text.contains("SASL") && text.contains("fail")) {
                return Ok(());
            }
        }
    }
    anyhow::bail!("Timed out waiting for SASL failure")
}
