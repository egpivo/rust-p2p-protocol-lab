use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

impl NodeId {
    pub fn random() -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut h = DefaultHasher::new();
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
            .hash(&mut h);
        std::thread::current().id().hash(&mut h);
        NodeId(h.finish())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Hello {
        node_id: NodeId,
        listen_addr: SocketAddr,
        peers: Vec<SocketAddr>,
    },
    Ping,
    Pong,
    GetPeers,
    Peers(Vec<SocketAddr>),
    Tip {
        height: u64,
        hash: String,
    },
}
