use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::sleep;

const SERVER_ADDR: &str = "127.0.0.1:6667";
const CHANNEL_GENERAL: &str = "#swarm_general";
const CHANNEL_ADMIN: &str = "#swarm_admin";
const SWARM_SIZE: usize = 10;

#[derive(Clone, Copy, Debug)]
enum Role {
    Admin,   // Client 0: Opers up, sets modes, kicks
    Chatter, // Client 1-7: Chats, joins general
    Lurker,  // Client 8-9: Invisible, watches
}

impl Role {
    fn from_id(id: usize) -> Self {
        match id {
            0 => Role::Admin,
            8 | 9 => Role::Lurker,
            _ => Role::Chatter,
        }
    }
}

async fn run_client(id: usize, role: Role) -> std::io::Result<()> {
    let nick = format!("SwarmBot{}", id);
    let username = format!("bot{}", id);
    let realname = format!("Swarm Test Bot {} ({:?})", id, role);

    println!("[{}] Connecting as {:?}...", nick, role);
    let stream = TcpStream::connect(SERVER_ADDR).await?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = write_half;

    // Registration
    let reg_cmd = format!("NICK {}\r\nUSER {} 0 * :{}\r\n", nick, username, realname);
    writer.write_all(reg_cmd.as_bytes()).await?;

    // Specific startup actions based on Role
    sleep(Duration::from_millis(500 + (id as u64 * 100))).await;

    match role {
        Role::Admin => {
            // Oper up
            println!("[{}] Attempting OPER...", nick);
            writer.write_all(b"OPER admin password\r\n").await?;
            // Join Admin Channel
            writer
                .write_all(format!("JOIN {}\r\n", CHANNEL_ADMIN).as_bytes())
                .await?;
            sleep(Duration::from_millis(200)).await;
            // Set Key and Topic
            writer
                .write_all(format!("MODE {} +k secret\r\n", CHANNEL_ADMIN).as_bytes())
                .await?;
            writer
                .write_all(
                    format!("TOPIC {} :Restricted Area for Operators\r\n", CHANNEL_ADMIN)
                        .as_bytes(),
                )
                .await?;
            // Also join General to moderate
            writer
                .write_all(format!("JOIN {}\r\n", CHANNEL_GENERAL).as_bytes())
                .await?;
            sleep(Duration::from_millis(200)).await;
            writer
                .write_all(format!("MODE {} +nt\r\n", CHANNEL_GENERAL).as_bytes())
                .await?;
        }
        Role::Chatter => {
            // Join General
            writer
                .write_all(format!("JOIN {}\r\n", CHANNEL_GENERAL).as_bytes())
                .await?;
            // Set Away status
            if id % 2 == 0 {
                writer.write_all(b"AWAY :I am a busy bot\r\n").await?;
            }
        }
        Role::Lurker => {
            // Set Invisible
            writer.write_all(b"MODE SwarmBot8 +i\r\n").await?;
            // Join General
            writer
                .write_all(format!("JOIN {}\r\n", CHANNEL_GENERAL).as_bytes())
                .await?;
            // List channels
            writer.write_all(b"LIST\r\n").await?;
        }
    }

    let mut line = String::new();
    let mut step = 0;

    loop {
        line.clear();
        tokio::select! {
             byte_count = reader.read_line(&mut line) => {
                if byte_count? == 0 {
                    println!("[{}] Disconnected", nick);
                    break;
                }

                let trim_line = line.trim();

                // Handle PING
                if trim_line.starts_with("PING") {
                    let token = trim_line.split_whitespace().nth(1).unwrap_or("");
                    let pong = format!("PONG {}\r\n", token);
                    writer.write_all(pong.as_bytes()).await?;
                }

                // Parsed Output for Verification
                if trim_line.contains(" 381 ") {
                     println!("[{}] \x1b[32mVERIFIED: You are now an IRC Operator\x1b[0m", nick);
                }
                if trim_line.contains(" 332 ") {
                     println!("[{}] VERIFIED: Topic observed: {}", nick, trim_line);
                }
                if trim_line.contains(" INVITE ") {
                     println!("[{}] VERIFIED: Received Invite", nick);
                     // Auto-accept invites
                     let parts: Vec<&str> = trim_line.split_whitespace().collect();
                     if let Some(chan) = parts.last() {
                         let join = format!("JOIN {}\r\n", chan.trim_start_matches(':'));
                         writer.write_all(join.as_bytes()).await?;
                     }
                }
                if trim_line.contains(" KICK ") {
                     println!("[{}] NOTICE: Kicked from channel: {}", nick, trim_line);
                     // Rejoin revenge! (After delay)
                     sleep(Duration::from_secs(2)).await;
                     writer.write_all(format!("JOIN {}\r\n", CHANNEL_GENERAL).as_bytes()).await?;
                }
            }

            // Periodic Actions
            _ = sleep(Duration::from_secs(3 + (id as u64 % 5))) => {
                step += 1;
                match role {
                    Role::Admin => {
                        if step % 5 == 0 {
                            // Random kick of chatter
                            let target_id = (step % 7) + 1; // 1-7 (Chatters)
                            let kick_cmd = format!("KICK {} SwarmBot{} :Admin abuse test\r\n", CHANNEL_GENERAL, target_id);
                            writer.write_all(kick_cmd.as_bytes()).await?;
                        } else if step % 7 == 0 {
                            // Invite Lurker to Admin
                            let invite_cmd = format!("INVITE SwarmBot8 {}\r\n", CHANNEL_ADMIN);
                             writer.write_all(invite_cmd.as_bytes()).await?;
                        }
                    }
                    Role::Chatter => {
                         let msg = format!("PRIVMSG {} :[Step {}] Hello from {}\r\n", CHANNEL_GENERAL, step, nick);
                         writer.write_all(msg.as_bytes()).await?;

                         // Occasional WHOIS
                         if step % 10 == 0 {
                             writer.write_all(b"WHOIS SwarmBot0\r\n").await?;
                         }
                    }
                    Role::Lurker => {
                        // Lurkers are quiet, but send WHO occasionally
                        if step % 20 == 0 {
                            writer.write_all(format!("WHO {}\r\n", CHANNEL_GENERAL).as_bytes()).await?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    println!("Starting Enhanced Swarm Simulator ({} Clients)", SWARM_SIZE);
    println!("Roles: [0:Admin] [1-7:Chatter] [8-9:Lurker]");

    let mut set = tokio::task::JoinSet::new();

    for i in 0..SWARM_SIZE {
        set.spawn(async move {
            let role = Role::from_id(i);
            // Stagger start
            sleep(Duration::from_millis(i as u64 * 1200)).await;
            if let Err(e) = run_client(i, role).await {
                eprintln!("[Client {}] Error: {}", i, e);
            }
        });
    }

    tokio::signal::ctrl_c().await?;
    println!("Shutting down swarm...");
    set.abort_all();
    Ok(())
}
