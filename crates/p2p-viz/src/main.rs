use axum::{
    Json, Router,
    extract::{Path, State},
    response::{Html, Sse, sse::Event},
    routing::{get, post},
};
use p2p_core::NodeEvent;
use p2p_env::{EclipseAttack, EnvConfig, NetworkEnv, NetworkPartitionAttack, SybilAttack};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::cors::CorsLayer;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SimConfig {
    max_peers: usize,
    sybil_default: usize,
    eclipse_default: usize,
    partition_default: usize,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            max_peers: 20,
            sybil_default: 7,
            eclipse_default: 6,
            partition_default: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SimEvent {
    NodeSpawned {
        id: String,
        addr: String,
        role: String,
    },
    ConnectionEstablished {
        from: String,
        to: String,
    },
    ConnectionDropped {
        from: String,
        to: String,
    },
    PacketSent {
        from: String,
        to: String,
        data: String,
    },
    MitmIntercept {
        node: String,
        data: String,
    },
    HandshakeFailed {
        from: String,
        reason: String,
    },
    PeerVerified {
        from: String,
        remote: String,
    },
    PeerSlotsFull {
        node: String,
    },
    NetworkSplit {
        group_a: Vec<String>,
        group_b: Vec<String>,
    },
    ScenarioComplete {
        scenario: String,
    },
    ScenarioReset,
}

type Tx = broadcast::Sender<SimEvent>;

#[derive(Debug, Deserialize, Default)]
struct RunParams {
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    max_peers: Option<usize>,
}

#[derive(Clone)]
struct AppState {
    tx: Arc<Tx>,
    cfg: Arc<SimConfig>,
}

#[tokio::main]
async fn main() {
    let cfg: SimConfig = std::fs::read_to_string("crates/p2p-viz/config.toml")
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();

    let (tx, _) = broadcast::channel::<SimEvent>(64);
    let state = AppState {
        tx: Arc::new(tx),
        cfg: Arc::new(cfg),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/config", get(config_handler))
        .route("/events", get(sse_handler))
        .route("/run/{scenario}", post(run_scenario))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = "127.0.0.1:3000";
    println!("p2p-viz listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn config_handler(State(state): State<AppState>) -> Json<SimConfig> {
    Json((*state.cfg).clone())
}

async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result: Result<SimEvent, _>| {
        result.ok().map(|event| {
            let data = serde_json::to_string(&event).unwrap();
            Ok(Event::default().data(data))
        })
    });
    Sse::new(stream)
}

async fn run_scenario(
    Path(scenario): Path<String>,
    State(state): State<AppState>,
    Json(params): Json<RunParams>,
) {
    let tx = state.tx.clone();
    let cfg = state.cfg.clone();
    tokio::spawn(async move {
        match scenario.as_str() {
            "tcp" => run_tcp_scenario(&tx).await,
            "tls-mitm" => run_tls_mitm_scenario(&tx).await,
            "noise" => run_noise_scenario(&tx).await,
            "sybil" => {
                run_sybil_scenario(
                    &tx,
                    params.count.unwrap_or(cfg.sybil_default),
                    params.max_peers.unwrap_or(cfg.max_peers),
                )
                .await
            }
            "eclipse" => {
                run_eclipse_scenario(
                    &tx,
                    params.count.unwrap_or(cfg.eclipse_default),
                    cfg.max_peers,
                )
                .await
            }
            "partition" => {
                run_partition_scenario(
                    &tx,
                    params.count.unwrap_or(cfg.partition_default),
                    cfg.max_peers,
                )
                .await
            }
            _ => {}
        }
    });
}

async fn run_tcp_scenario(tx: &Tx) {
    use SimEvent::*;
    let delay = tokio::time::Duration::from_millis(600);

    tx.send(ScenarioReset).ok();
    tokio::time::sleep(delay).await;

    tx.send(NodeSpawned {
        id: "Alice".into(),
        addr: "9000".into(),
        role: "honest".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(NodeSpawned {
        id: "Bob".into(),
        addr: "9001".into(),
        role: "honest".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(NodeSpawned {
        id: "Mallory".into(),
        addr: "9999".into(),
        role: "attacker".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    tx.send(ConnectionEstablished {
        from: "Alice".into(),
        to: "Mallory".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(ConnectionEstablished {
        from: "Mallory".into(),
        to: "Bob".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    tx.send(PacketSent {
        from: "Alice".into(),
        to: "Mallory".into(),
        data: r#"{"Hello":{"node_id":1234}}"#.into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(MitmIntercept {
        node: "Mallory".into(),
        data: r#"{"Hello":{"node_id":1234}}"#.into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(PacketSent {
        from: "Mallory".into(),
        to: "Bob".into(),
        data: r#"{"Hello":{"node_id":1234}}"#.into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    tx.send(ScenarioComplete {
        scenario: "tcp".into(),
    })
    .ok();
}

async fn run_tls_mitm_scenario(tx: &Tx) {
    use SimEvent::*;
    let delay = tokio::time::Duration::from_millis(600);

    tx.send(ScenarioReset).ok();
    tokio::time::sleep(delay).await;

    tx.send(NodeSpawned {
        id: "Alice".into(),
        addr: "9000".into(),
        role: "honest".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(NodeSpawned {
        id: "Bob".into(),
        addr: "9001".into(),
        role: "honest".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(NodeSpawned {
        id: "Mallory".into(),
        addr: "9999".into(),
        role: "attacker".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    tx.send(ConnectionEstablished {
        from: "Alice".into(),
        to: "Mallory".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(ConnectionEstablished {
        from: "Mallory".into(),
        to: "Bob".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    // TLS established on both sides — but two separate sessions
    tx.send(PacketSent {
        from: "Alice".into(),
        to: "Mallory".into(),
        data: "[TLS] Hello encrypted".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(MitmIntercept {
        node: "Mallory".into(),
        data: r#"{"Hello":{"node_id":1234}}"#.into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(PacketSent {
        from: "Mallory".into(),
        to: "Bob".into(),
        data: "[TLS] Hello re-encrypted".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    tx.send(ScenarioComplete {
        scenario: "tls-mitm".into(),
    })
    .ok();
}

async fn run_noise_scenario(tx: &Tx) {
    use SimEvent::*;
    let delay = tokio::time::Duration::from_millis(600);

    tx.send(ScenarioReset).ok();
    tokio::time::sleep(delay).await;

    tx.send(NodeSpawned {
        id: "Alice".into(),
        addr: "9000".into(),
        role: "honest".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(NodeSpawned {
        id: "Bob".into(),
        addr: "9001".into(),
        role: "honest".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(NodeSpawned {
        id: "Mallory".into(),
        addr: "9999".into(),
        role: "attacker".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    // Alice tries to connect through Mallory
    tx.send(ConnectionEstablished {
        from: "Alice".into(),
        to: "Mallory".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    // Noise handshake — identity mismatch detected
    tx.send(HandshakeFailed {
        from: "Alice".into(),
        reason: "MITM DETECTED: remote key mismatch".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(ConnectionDropped {
        from: "Alice".into(),
        to: "Mallory".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    // Alice connects directly to Bob
    tx.send(ConnectionEstablished {
        from: "Alice".into(),
        to: "Bob".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(PeerVerified {
        from: "Alice".into(),
        remote: "Bob".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;
    tx.send(PacketSent {
        from: "Alice".into(),
        to: "Bob".into(),
        data: "[Noise] Hello encrypted + authenticated".into(),
    })
    .ok();
    tokio::time::sleep(delay).await;

    tx.send(ScenarioComplete {
        scenario: "noise".into(),
    })
    .ok();
}

async fn run_sybil_scenario(tx: &Tx, count: usize, max_peers: usize) {
    use SimEvent::*;
    let d = tokio::time::Duration::from_millis;

    tx.send(ScenarioReset).ok();
    tokio::time::sleep(d(400)).await;

    // spawn real honest network (4 nodes, base port 8100)
    let port_map = (0..4usize)
        .map(|i| (8100 + i as u16, format!("N{}", i + 1)))
        .collect();
    let node_tx = make_event_forwarder(tx.clone(), port_map);
    let mut env = NetworkEnv::new(EnvConfig {
        honest_nodes: 4,
        max_peers,
        base_port: 8100,
    });
    env.reset(Some(node_tx)).await;

    for i in 0..4usize {
        let port = 8100 + i as u16;
        tx.send(NodeSpawned {
            id: format!("N{}", i + 1),
            addr: port.to_string(),
            role: "honest".into(),
        })
        .ok();
        tokio::time::sleep(d(300)).await;
    }
    for i in 2..=4usize {
        tx.send(ConnectionEstablished {
            from: format!("N{i}"),
            to: "N1".into(),
        })
        .ok();
        tokio::time::sleep(d(200)).await;
    }
    tokio::time::sleep(d(500)).await;

    let attack = SybilAttack {
        target_index: 0,
        count,
    };

    for i in 1..=count {
        let id = format!("S{i}");
        tx.send(NodeSpawned {
            id: id.clone(),
            addr: format!("1900{i}"),
            role: "sybil".into(),
        })
        .ok();
        tokio::time::sleep(d(300)).await;
        tx.send(ConnectionEstablished {
            from: id,
            to: "N1".into(),
        })
        .ok();
        tokio::time::sleep(d(200)).await;
    }

    let result = attack.execute(&env).await;
    let occupancy = result.metrics.get("occupancy").copied().unwrap_or(0.0);
    let sybil_count = result.metrics.get("sybil_peers").copied().unwrap_or(0.0) as usize;
    let total_peers = result
        .metrics
        .get("total_peers")
        .copied()
        .unwrap_or(max_peers as f64) as usize;
    let honest_count = total_peers.saturating_sub(sybil_count);

    tx.send(PeerSlotsFull { node: "N1".into() }).ok();
    tokio::time::sleep(d(400)).await;
    tx.send(ConnectionDropped {
        from: "N1".into(),
        to: "N2".into(),
    })
    .ok();
    tokio::time::sleep(d(400)).await;

    tx.send(ScenarioComplete {
        scenario: format!(
            "sybil — {count} identities, {sybil_count} fake peers, {honest_count} honest peers, {occupancy:.1}% occupancy"
        )
    }).ok();
}

async fn run_eclipse_scenario(tx: &Tx, count: usize, max_peers: usize) {
    use SimEvent::*;
    let d = tokio::time::Duration::from_millis;

    tx.send(ScenarioReset).ok();
    tokio::time::sleep(d(400)).await;

    let mut env = NetworkEnv::new(EnvConfig {
        honest_nodes: 4,
        max_peers,
        base_port: 8200,
    });
    env.reset(None).await;

    let names = ["Victim", "H1", "H2", "H3"];
    for (i, name) in names.iter().enumerate() {
        let port = 8200 + i as u16;
        let role = if i == 0 { "victim" } else { "honest" };
        tx.send(NodeSpawned {
            id: name.to_string(),
            addr: port.to_string(),
            role: role.into(),
        })
        .ok();
        tokio::time::sleep(d(300)).await;
    }
    for h in ["H1", "H2", "H3"] {
        tx.send(ConnectionEstablished {
            from: "Victim".into(),
            to: h.into(),
        })
        .ok();
        tokio::time::sleep(d(200)).await;
    }
    tokio::time::sleep(d(500)).await;

    let attack = EclipseAttack {
        target_index: 0,
        sybil_count: count,
    };

    for i in 1..=count {
        let id = format!("A{i}");
        tx.send(NodeSpawned {
            id: id.clone(),
            addr: format!("8800{i}"),
            role: "attacker".into(),
        })
        .ok();
        tokio::time::sleep(d(300)).await;
        tx.send(ConnectionEstablished {
            from: id,
            to: "Victim".into(),
        })
        .ok();
        tokio::time::sleep(d(200)).await;
    }

    let result = attack.execute(&env).await;
    let isolated = result
        .metrics
        .get("honest_peers_remaining")
        .copied()
        .unwrap_or(0.0) as usize
        == 0;

    tokio::time::sleep(d(400)).await;
    for h in ["H1", "H2", "H3"] {
        tx.send(ConnectionDropped {
            from: "Victim".into(),
            to: h.into(),
        })
        .ok();
        tokio::time::sleep(d(350)).await;
    }
    if isolated {
        tx.send(PeerSlotsFull {
            node: "Victim".into(),
        })
        .ok();
    }
    tokio::time::sleep(d(400)).await;

    tx.send(ScenarioComplete {
        scenario: format!("eclipse — victim isolated: {isolated}"),
    })
    .ok();
}

async fn run_partition_scenario(tx: &Tx, per_group: usize, max_peers: usize) {
    use SimEvent::*;
    let d = tokio::time::Duration::from_millis;

    let total = per_group * 2;

    tx.send(ScenarioReset).ok();
    tokio::time::sleep(d(400)).await;

    let mut env = NetworkEnv::new(EnvConfig {
        honest_nodes: total,
        max_peers,
        base_port: 8300,
    });
    env.reset(None).await;

    for i in 0..total {
        let port = 8300 + i as u16;
        tx.send(NodeSpawned {
            id: format!("P{}", i + 1),
            addr: port.to_string(),
            role: "honest".into(),
        })
        .ok();
        tokio::time::sleep(d(250)).await;
    }
    // ring connections within each group + a few cross-group links
    for i in 0..total {
        let a = format!("P{}", i + 1);
        let b = format!("P{}", (i + 1) % total + 1);
        tx.send(ConnectionEstablished { from: a, to: b }).ok();
        tokio::time::sleep(d(200)).await;
    }
    tokio::time::sleep(d(500)).await;

    let group_a: Vec<usize> = (0..per_group).collect();
    let group_b: Vec<usize> = (per_group..total).collect();
    let attack = NetworkPartitionAttack {
        group_a: group_a.clone(),
        group_b: group_b.clone(),
    };
    attack.execute(&env).await;

    // drop cross-group edges
    for &a in &group_a {
        for &b in &group_b {
            let pa = format!("P{}", a + 1);
            let pb = format!("P{}", b + 1);
            tx.send(ConnectionDropped { from: pa, to: pb }).ok();
            tokio::time::sleep(d(300)).await;
        }
    }

    let ga_names: Vec<String> = group_a.iter().map(|&i| format!("P{}", i + 1)).collect();
    let gb_names: Vec<String> = group_b.iter().map(|&i| format!("P{}", i + 1)).collect();
    tx.send(NetworkSplit {
        group_a: ga_names,
        group_b: gb_names,
    })
    .ok();
    tokio::time::sleep(d(400)).await;

    tx.send(ScenarioComplete {
        scenario: format!("partition — {per_group}|{per_group} split"),
    })
    .ok();
}

fn make_event_forwarder(
    tx: Tx,
    port_map: std::collections::HashMap<u16, String>,
) -> mpsc::UnboundedSender<NodeEvent> {
    let (node_tx, mut node_rx) = mpsc::unbounded_channel::<NodeEvent>();
    tokio::spawn(async move {
        while let Some(ev) = node_rx.recv().await {
            let id_for = |addr: std::net::SocketAddr| -> String {
                port_map
                    .get(&addr.port())
                    .cloned()
                    .unwrap_or_else(|| addr.port().to_string())
            };
            match ev {
                NodeEvent::PeerConnected { addr: _, .. } => {
                    // tx.send(SimEvent::ConnectionEstablished {
                    //     from: id_for(addr),
                    //     to: "network".into(),
                    // }).ok();
                }
                NodeEvent::PeerDisconnected { addr: _ } => {
                    // tx.send(SimEvent::ConnectionDropped {
                    //     from: id_for(addr),
                    //     to: "network".into(),
                    // }).ok();
                }
                NodeEvent::MessageReceived { from, msg } => {
                    if let Some(id) = port_map.get(&from.port()) {
                        tx.send(SimEvent::PacketSent {
                            from: id.clone(),
                            to: "network".into(),
                            data: format!("{msg:?}"),
                        })
                        .ok();
                    }
                }
                NodeEvent::HandshakeFailed { addr, reason } => {
                    tx.send(SimEvent::HandshakeFailed {
                        from: id_for(addr),
                        reason,
                    })
                    .ok();
                }
            }
        }
    });
    node_tx
}
