use p2p_core::{Message, NodeEvent, NodeId};
use p2p_node::{PeerList, handle_inbound};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;

pub struct EnvConfig {
    pub honest_nodes: usize,
    pub max_peers: usize,
    pub base_port: u16, // will be port, port + 1, ...
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            honest_nodes: 6,
            max_peers: 8,
            base_port: 9000,
        }
    }
}

pub struct AttackResult {
    pub success: bool,
    pub metrics: HashMap<String, f64>,
    pub summary: String,
}

pub trait Attack: Send + Sync {
    fn name(&self) -> &str;
}

pub struct NetworkEnv {
    pub config: EnvConfig,
    nodes: Vec<NodeHandle>,
}

impl Drop for NetworkEnv {
    fn drop(&mut self) {
        for node in &self.nodes {
            node.task.abort();
        }
    }
}

impl NetworkEnv {
    pub fn new(config: EnvConfig) -> Self {
        Self {
            config,
            nodes: Vec::new(),
        }
    }

    pub async fn reset(&mut self, event_tx: Option<UnboundedSender<NodeEvent>>) {
        // kill old tasks
        for node in self.nodes.drain(..) {
            node.task.abort();
        }

        // spawn fresh nodes
        let seed = format!("127.0.0.1:{}", self.config.base_port)
            .parse()
            .unwrap();

        for i in 0..self.config.honest_nodes {
            let port = self.config.base_port + i as u16;
            let seeds = if i == 0 { vec![] } else { vec![seed] };
            let (task, blocklist) =
                spawn_node(port, seeds, self.config.max_peers, event_tx.clone()).await;
            let addr = format!("127.0.0.1:{port}").parse().unwrap();
            self.nodes.push(NodeHandle {
                port,
                addr,
                blocklist,
                task,
            });
        }
    }

    pub async fn run(&self, _attack: &dyn Attack) -> AttackResult {
        todo!()
    }
}

pub struct NodeHandle {
    pub port: u16,
    pub addr: SocketAddr,
    pub blocklist: p2p_node::BlockList,
    task: tokio::task::JoinHandle<()>,
}

pub async fn spawn_node(
    port: u16,
    seeds: Vec<SocketAddr>,
    max_peers: usize,
    event_tx: Option<UnboundedSender<NodeEvent>>,
) -> (tokio::task::JoinHandle<()>, p2p_node::BlockList) {
    let blocklist: p2p_node::BlockList = Arc::new(Mutex::new(std::collections::HashSet::new()));
    let blocklist_inner = blocklist.clone();

    let handle = tokio::spawn(async move {
        let node_id = NodeId::random();
        let peers: PeerList = Arc::new(Mutex::new(Vec::new()));
        let listen_addr = format!("127.0.0.1:{port}");
        let listener = TcpListener::bind(&listen_addr).await.unwrap();

        // connect to seeds
        for seed in seeds {
            let peers = peers.clone();
            tokio::spawn(async move {
                // retry with backoff: seed listener may not be bound yet when all nodes spawn together
                let mut stream = None;
                for attempt in 0..8u64 {
                    if attempt > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(50 * attempt)).await;
                    }
                    if let Ok(s) = TcpStream::connect(seed).await {
                        stream = Some(s);
                        break;
                    }
                }
                let Some(mut stream) = stream else { return };

                let known = peers.lock().unwrap().clone();
                let mut msg = serde_json::to_string(&Message::Hello {
                    node_id,
                    listen_addr: format!("127.0.0.1:{port}").parse().unwrap(),
                    peers: known,
                })
                .unwrap();
                msg.push('\n');
                if stream.write_all(msg.as_bytes()).await.is_err() {
                    return;
                }

                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut line = String::new();

                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    return;
                }
                if let Ok(Message::Hello { .. }) = serde_json::from_str(line.trim()) {
                    peers.lock().unwrap().push(seed);
                }

                let mut tick = 0u32;
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    tick += 1;
                    let mut ping = serde_json::to_string(&Message::Ping).unwrap();
                    ping.push('\n');
                    if write_half.write_all(ping.as_bytes()).await.is_err() {
                        break;
                    }
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }

                    if tick.is_multiple_of(2) {
                        line.clear();
                        let mut get = serde_json::to_string(&Message::GetPeers).unwrap();
                        get.push('\n');
                        if write_half.write_all(get.as_bytes()).await.is_err() {
                            break;
                        }
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            break;
                        }
                        if let Ok(Message::Peers(list)) = serde_json::from_str(line.trim()) {
                            let mut p = peers.lock().unwrap();
                            for addr in list {
                                if p.len() < max_peers && !p.contains(&addr) {
                                    p.push(addr);
                                }
                            }
                        }
                    }
                }
            });
        }

        // accept inbound
        loop {
            if let Ok((stream, peer_addr)) = listener.accept().await {
                let peers = peers.clone();
                let bl = blocklist_inner.clone();
                tokio::spawn(handle_inbound(
                    stream,
                    peer_addr,
                    node_id,
                    port,
                    peers,
                    bl,
                    max_peers,
                    event_tx.clone(),
                ));
            }
        }
    });
    (handle, blocklist)
}

pub struct SybilAttack {
    pub target_index: usize,
    pub count: usize,
}

impl SybilAttack {
    pub async fn execute(&self, env: &NetworkEnv) -> AttackResult {
        let target_addr = env.nodes[self.target_index].addr;
        let count = self.count;

        // spawn Sybil nodes
        for i in 0..count {
            tokio::spawn(async move {
                let Some((_reader, _writer)) =
                    connect_as_sybil(target_addr, 19000 + i as u16).await
                else {
                    return;
                };
                tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            });
        }

        // wait for connections to settle
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // measure occupancy via query_peers
        let peers: Vec<SocketAddr> = query_peers(target_addr).await.unwrap_or_default();
        let sybil_count = peers.iter().filter(|a| a.port() >= 19000).count();
        let occupancy = sybil_count as f64 / peers.len().max(1) as f64 * 100.0;

        let mut metrics = HashMap::new();
        metrics.insert("occupancy".to_string(), occupancy);
        metrics.insert("total_peers".to_string(), peers.len() as f64);
        metrics.insert("sybil_peers".to_string(), sybil_count as f64);

        AttackResult {
            success: occupancy >= 100.0,
            metrics,
            summary: format!(
                "Sybil occupancy: {occupancy:.0}% ({sybil_count}/{} peers)",
                peers.len()
            ),
        }
    }
}

impl Attack for SybilAttack {
    fn name(&self) -> &str {
        "Sybil"
    }
}

async fn query_peers(addr: SocketAddr) -> Option<Vec<SocketAddr>> {
    let mut stream = TcpStream::connect(addr).await.ok()?;
    let node_id = NodeId::random();
    let mut line = serde_json::to_string(&Message::Hello {
        node_id,
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        peers: vec![],
    })
    .unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).await.ok()?;

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await.ok()?;
    line.clear();

    let mut get = serde_json::to_string(&Message::GetPeers).unwrap();
    get.push('\n');
    write_half.write_all(get.as_bytes()).await.ok()?;
    reader.read_line(&mut line).await.ok()?;
    match serde_json::from_str(line.trim()).ok()? {
        Message::Peers(list) => Some(list),
        _ => None,
    }
}

async fn connect_as_sybil(
    target: SocketAddr,
    port: u16,
) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
    let node_id = NodeId::random();
    let Ok(mut stream) = TcpStream::connect(target).await else {
        return None;
    };
    let mut msg = serde_json::to_string(&Message::Hello {
        node_id,
        listen_addr: format!("127.0.0.1:{port}").parse().unwrap(),
        peers: vec![],
    })
    .unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).await.ok()?;

    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut buf = String::new();
    reader.read_line(&mut buf).await.ok();

    Some((reader, write_half))
}

pub struct EclipseAttack {
    pub target_index: usize,
    pub sybil_count: usize,
}

impl EclipseAttack {
    pub async fn execute(&self, env: &NetworkEnv) -> AttackResult {
        let target_addr = env.nodes[self.target_index].addr;
        let count = self.sybil_count;

        for i in 0..count {
            tokio::spawn(async move {
                let Some((_reader, mut write_half)) =
                    connect_as_sybil(target_addr, 19000 + i as u16).await
                else {
                    return;
                };
                let mut tip = serde_json::to_string(&Message::Tip {
                    height: 9999,
                    hash: "attacker_tip_FAKE".to_string(),
                })
                .unwrap();
                tip.push('\n');
                write_half.write_all(tip.as_bytes()).await.ok();

                tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            });
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let peers: Vec<SocketAddr> = query_peers(target_addr).await.unwrap_or_default();
        let sybil = peers.iter().filter(|a| a.port() >= 19000).count();
        let occupancy = sybil as f64 / peers.len().max(1) as f64 * 100.0;
        let diverged = sybil == peers.len();

        let mut metrics = HashMap::new();
        metrics.insert("occupancy".to_string(), occupancy);
        metrics.insert("sybil_peers".to_string(), sybil as f64);
        metrics.insert("honest_peers".to_string(), (peers.len() - sybil) as f64);
        metrics.insert(
            "state_diverged".to_string(),
            if diverged { 1.0 } else { 0.0 },
        );

        AttackResult {
            success: diverged,
            metrics,
            summary: format!(
                "Eclipse: {sybil}/{} peers Sybil, state_diverged={diverged}",
                peers.len(),
            ),
        }
    }
}

impl Attack for EclipseAttack {
    fn name(&self) -> &str {
        "Eclipse"
    }
}

pub struct NetworkPartitionAttack {
    pub group_a: Vec<usize>,
    pub group_b: Vec<usize>,
}

impl NetworkPartitionAttack {
    pub async fn execute(&self, env: &NetworkEnv) -> AttackResult {
        // collect IPs of each group
        let addrs_a: Vec<_> = self.group_a.iter().map(|&i| env.nodes[i].addr).collect();
        let addrs_b: Vec<_> = self.group_b.iter().map(|&i| env.nodes[i].addr).collect();

        // block group_b IPs on all group_a nodes
        for &i in &self.group_a {
            let mut bl = env.nodes[i].blocklist.lock().unwrap();
            for addr in &addrs_b {
                bl.insert(addr.ip());
            }
        }

        // block group_a IPs on all group_b nodes
        for &i in &self.group_b {
            let mut bl = env.nodes[i].blocklist.lock().unwrap();
            for addr in &addrs_a {
                bl.insert(addr.ip());
            }
        }

        let mut metrics = HashMap::new();
        metrics.insert("group_a_size".to_string(), self.group_a.len() as f64);
        metrics.insert("group_b_size".to_string(), self.group_b.len() as f64);

        // wait for partition to take effect
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // verify: query group_a node, check if group_b addrs still appear
        let a_peers: Vec<SocketAddr> = query_peers(addrs_a[0]).await.unwrap_or_default();
        let cross_peers = a_peers
            .iter()
            .filter(|p| addrs_b.iter().any(|b| b.port() == p.port()))
            .count();

        metrics.insert("cross_peers_remaining".to_string(), cross_peers as f64);
        let partitioned = cross_peers == 0;

        AttackResult {
            success: partitioned,
            metrics,
            summary: format!(
                "Network partitioned: group_a={:?} isolated from group_b={:?}",
                self.group_a, self.group_b,
            ),
        }
    }
}

impl Attack for NetworkPartitionAttack {
    fn name(&self) -> &str {
        "NetworkPartition"
    }
}

async fn bootstrap_from_seed(seed_addr: SocketAddr) -> Vec<SocketAddr> {
    let Ok(stream) = TcpStream::connect(seed_addr).await else {
        return vec![];
    };
    let node_id = NodeId::random();
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    // send Hello
    let mut msg = serde_json::to_string(&Message::Hello {
        node_id,
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        peers: vec![],
    })
    .unwrap();
    msg.push('\n');
    if write_half.write_all(msg.as_bytes()).await.is_err() {
        return vec![];
    }

    // read Hello
    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return vec![];
    }
    line.clear();

    // send GetPeers
    let mut get = serde_json::to_string(&Message::GetPeers).unwrap();
    get.push('\n');
    if write_half.write_all(get.as_bytes()).await.is_err() {
        return vec![];
    }

    // read Peers response
    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return vec![];
    }
    match serde_json::from_str(line.trim()) {
        Ok(Message::Peers(list)) => list,
        _ => vec![],
    }
}

async fn run_poisoned_seed(
    listener: TcpListener,
    listen_addr: SocketAddr,
    sybil_addrs: Vec<SocketAddr>,
) {
    while let Ok((stream, _)) = listener.accept().await {
        let sybil_addrs = sybil_addrs.clone();
        tokio::spawn(async move {
            let node_id = NodeId::random();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            // read Hello
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                return;
            }

            // reply Hello
            let mut reply = serde_json::to_string(&Message::Hello {
                node_id,
                listen_addr,
                peers: vec![],
            })
            .unwrap();
            reply.push('\n');
            if write_half.write_all(reply.as_bytes()).await.is_err() {
                return;
            }

            // wait for GetPeers -> serve poisoned list
            line.clear();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                return;
            }
            if let Ok(Message::GetPeers) = serde_json::from_str(line.trim()) {
                let mut resp = serde_json::to_string(&Message::Peers(sybil_addrs)).unwrap();
                resp.push('\n');
                write_half.write_all(resp.as_bytes()).await.ok();
            }
        });
    }
}

pub struct BootstrapPoisoningAttack {
    pub sybil_count: usize,
}

impl BootstrapPoisoningAttack {
    pub async fn execute(&self, honest_env: &NetworkEnv) -> AttackResult {
        let sybil_count = self.sybil_count;
        let _max_peers = honest_env.config.max_peers;

        // fake sybil addresses
        let sybil_addrs: Vec<SocketAddr> = (0..sybil_count)
            .map(|i| format!("127.0.0.1:{}", 19100 + i as u16).parse().unwrap())
            .collect();

        // Start a poisoned seed on an ephemeral port so repeated CLI/viz runs do not collide.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let poisoned_addr = listener.local_addr().unwrap();

        tokio::spawn(run_poisoned_seed(
            listener,
            poisoned_addr,
            sybil_addrs.clone(),
        ));
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // honest seed is the first node in env
        let honest_seed_addr: SocketAddr = format!("127.0.0.1:{}", honest_env.config.base_port)
            .parse()
            .unwrap();

        // Measure both bootstraps.
        let poisoned_peers = bootstrap_from_seed(poisoned_addr).await;
        let _honest_peers_list = bootstrap_from_seed(honest_seed_addr).await;

        let sybil_in = poisoned_peers
            .iter()
            .filter(|a| sybil_addrs.contains(a))
            .count();
        let total = poisoned_peers.len().max(1);
        let sybil_ratio = sybil_in as f64 / total as f64 * 100.0;
        let honest_in = poisoned_peers.len() - sybil_in;

        let mut metrics = HashMap::new();
        metrics.insert("bootstrap_sybil_ratio".to_string(), sybil_ratio);
        metrics.insert("sybil_peers".to_string(), sybil_in as f64);
        metrics.insert("honest_peers".to_string(), honest_in as f64);
        metrics.insert("total_bootstrap_peers".to_string(), total as f64);

        AttackResult {
            success: sybil_ratio >= 50.0,
            metrics,
            summary: format!(
                "bootstrap poisoned: {sybil_ratio:.1}% sybil ({sybil_in}/{total} peers)"
            ),
        }
    }
}

impl Attack for BootstrapPoisoningAttack {
    fn name(&self) -> &str {
        "BootstrapPoisoning"
    }
}
