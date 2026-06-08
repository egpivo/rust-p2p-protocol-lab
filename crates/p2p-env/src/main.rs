use p2p_env::{EnvConfig, NetworkEnv};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let honest: usize = flag(&args, "--honest").unwrap_or(6);
    let attack: &str = flag_str(&args, "--attack").unwrap_or("sybil");
    let target: usize = flag(&args, "--target").unwrap_or(0);
    let sybil_count: usize = flag(&args, "--sybil").unwrap_or(20);

    let config = EnvConfig {
        honest_nodes: honest,
        ..EnvConfig::default()
    };

    let mut env = NetworkEnv::new(config);
    println!("=== P2P Security Gym ===");
    println!("Spawning {} honest nodes...", honest);
    env.reset().await;

    // give nodes time to connect to each other
    tokio::time::sleep(Duration::from_secs(3)).await;
    println!("Network ready.\n");

    match attack {
        "sybil" => {
            println!("Attack: Sybil");
            println!("Target node: {}", target);
            println!("Sybil count: {}", sybil_count);
            
            let attack = p2p_env::SybilAttack { target_index: target, count: sybil_count };
            let result = attack.execute(&env).await;

            println!("\n=== Result ===");
            println!("{}", result.summary);
            println!("Success: {}", result.success);
            for (k, v) in &result.metrics {
                println!("  {k}: {v:.1}");
            }
        }
        "eclipse" => {
            println!("Attack: Eclipse");
            println!("Target node: {}", target);
            println!("Sybil count: {}", sybil_count);
            
            let attack = p2p_env::EclipseAttack { target_index: target, sybil_count };
            let result = attack.execute(&env).await;

            println!("\n=== Result ===");
            println!("{}", result.summary);
            println!("Success: {}", result.success);
            for (k, v) in &result.metrics {
                println!("  {k}: {v:.1}");
            }
        }
        "partition" => {
            println!("Attack: NetworkPartition");

            // split nodes into two halves
            let half = honest / 2;
            let group_a: Vec<usize> = (0..half).collect();
            let group_b: Vec<usize> = (half..honest).collect();

            println!("Group A: {:?}", group_a);
            println!("Group B: {:?}", group_b);

            let attack = p2p_env::NetworkPartitionAttack { group_a, group_b };
            let result = attack.execute(&env).await;

            println!("\n=== Result ===");
            println!("{}", result.summary);
            println!("Success: {}", result.success);
            for (k, v) in &result.metrics {
                println!("  {k}: {v:.1}");
            }
        }
        other => {
            eprintln!("Unknown attack: {other}");
            std::process::exit(1);
        }
    }
}

fn flag<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.windows(2)
        .find(|w| w[0] == name)
        .and_then(|w| w[1].parse().ok())
}

fn flag_str<'a>(args: &'a[String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].as_str())
}

