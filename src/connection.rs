use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use futures::StreamExt;
use libp2p::gossipsub::{
    self, Gossipsub, GossipsubEvent, GossipsubMessage, IdentTopic, MessageAuthenticity, MessageId,
    Topic, ValidationMode,
};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{identity, mdns, Multiaddr, PeerId, Swarm};

#[derive(Debug, Clone)]
pub(crate) enum ConnectionMessage {
    Send(WireMessage),
    Dial(String),
    ChangeTopic(String),
}

#[derive(Debug, Clone)]
pub(crate) enum AppMessage {
    AddLogEntry(String),
    ReceivedWireMessage(Vec<u8>),
    PeerAdded((PeerId, Multiaddr)),
    PeerRemoved((PeerId, Multiaddr)),
}

/// Messages that are sent over the wire (network)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub(crate) enum WireMessage {
    /// Request a MergeDoc message from the peers
    RequestMerge,
    /// Sends the current document for merging and requests a MergeDoc message from the peers
    SyncDoc(Vec<u8>),
    /// Sends the current document for merging
    MergeDoc(Vec<u8>),
}

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
struct MyBehaviour {
    gossipsub: Gossipsub,
    mdns: mdns::async_io::Behaviour,
}

pub(crate) struct ConnectionState {
    topic: IdentTopic,
    swarm: Swarm<MyBehaviour>,
}

impl ConnectionState {
    /// Returns the initialized connection and some log entries
    pub(crate) async fn init<L>(topic_name: &str, msg_handler: L) -> anyhow::Result<Self>
    where
        L: Fn(AppMessage),
    {
        // Create a random PeerId
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        msg_handler(AppMessage::AddLogEntry(format!(
            "Local peer id: {local_peer_id}"
        )));

        // Set up an encrypted DNS-enabled TCP Transport over the Mplex protocol.
        let transport = libp2p::development_transport(local_key.clone()).await?;

        // To content-address message, we can take the hash of message and use it as an ID.
        let message_id_fn = |message: &GossipsubMessage| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            MessageId::from(s.finish().to_string())
        };

        // Set a custom gossipsub configuration
        let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
            .validation_mode(ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
            .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
            .build()
            .expect("Valid config");

        // build a gossipsub network behaviour
        let mut gossipsub =
            Gossipsub::new(MessageAuthenticity::Signed(local_key), gossipsub_config)
                .expect("Correct configuration");

        // Create a Gossipsub topic
        let topic = Topic::new(topic_name);

        // subscribes to our topic
        gossipsub.subscribe(&topic)?;

        // Create a Swarm to manage peers and events
        let mut swarm = {
            let mdns = mdns::async_io::Behaviour::new(mdns::Config::default())?;
            let behaviour = MyBehaviour { gossipsub, mdns };
            Swarm::with_async_std_executor(transport, behaviour, local_peer_id)
        };

        // Listen on all interfaces and whatever port the OS assigns
        let listen_addr = String::from("/ip4/0.0.0.0/tcp/0");
        swarm.listen_on(listen_addr.parse()?)?;

        Ok(Self { swarm, topic })
    }

    pub(crate) fn change_topic(&mut self, topic_name: impl Into<String>) -> anyhow::Result<()> {
        let topic_name: String = topic_name.into();

        if topic_name.is_empty() {
            return Err(anyhow::anyhow!("topic name can't be empty."));
        }

        // Returns Ok(false) if we were not subscribed anyway
        self.swarm
            .behaviour_mut()
            .gossipsub
            .unsubscribe(&self.topic)?;

        self.topic = Topic::new(topic_name);

        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topic)?;
        Ok(())
    }

    pub(crate) async fn run<F>(
        &mut self,
        mut msg_receiver: UnboundedReceiver<ConnectionMessage>,
        app_msg_handler: F,
    ) where
        F: Fn(AppMessage),
    {
        loop {
            futures::select! {
                msg = msg_receiver.select_next_some() => match msg {
                        ConnectionMessage::Send(data) => {
                            let v = match serde_json::to_vec(&data) {
                                Ok(v) => v,
                                Err(e) => {
                                    app_msg_handler(AppMessage::AddLogEntry(format!("failed to serialize wire message in topic: {}, Err: {e:?}", self.topic)));
                                    continue;
                                }
                            };
                            if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(self.topic.clone(), v) {
                                app_msg_handler(AppMessage::AddLogEntry(format!("failed to send wire message in topic: {}, Err: {e:?}", self.topic)));
                                continue;
                            }

                            app_msg_handler(AppMessage::AddLogEntry(format!("sent wire message in topic: {}", self.topic)));
                        }
                        ConnectionMessage::Dial(to_dial) => {
                            let addr = match to_dial.parse::<Multiaddr>() {
                                Ok(addr) => addr,
                                Err(e) => {
                                    app_msg_handler(AppMessage::AddLogEntry(format!("parsing dial address failed, Err: {e:?}")));
                                    continue;
                                }
                            };

                            if let Err(e) = self.swarm.dial(addr) {
                                app_msg_handler(AppMessage::AddLogEntry(format!("could not dial, Err: {e:?}")));
                                continue;
                            }

                            app_msg_handler(AppMessage::AddLogEntry(format!("dialed {to_dial:?}")));
                        }
                        ConnectionMessage::ChangeTopic(topic_name) => {
                            if let Err(e) = self.change_topic(&topic_name) {
                                app_msg_handler(AppMessage::AddLogEntry(format!("failed to change topic to: `{topic_name}`, Err: {e:?}")));
                                continue;
                            }

                            app_msg_handler(AppMessage::AddLogEntry(format!("changed topic to: {}", self.topic)));
                        }
                    }
                ,
                event = self.swarm.select_next_some() => match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                       app_msg_handler(AppMessage::AddLogEntry(format!("listening on {address:?}")));
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                        for (peer_id, multiaddr) in list {
                            self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);

                            app_msg_handler(AppMessage::PeerAdded((peer_id, multiaddr)));
                            app_msg_handler(AppMessage::AddLogEntry(format!("mDNS discovered a new peer: {peer_id}")));
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                        for (peer_id, multiaddr) in list {
                            self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);

                            app_msg_handler(AppMessage::PeerRemoved((peer_id, multiaddr)));
                            app_msg_handler(AppMessage::AddLogEntry(format!("mDNS discover peer has expired: {peer_id}")));
                        }
                    },
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(GossipsubEvent::Message {
                        propagation_source: peer_id,
                        message_id: id,
                        message,
                    })) => {
                        app_msg_handler(AppMessage::ReceivedWireMessage(message.data));
                        app_msg_handler(AppMessage::AddLogEntry(format!("got wire message, id: {id} from peer: {peer_id}")));
                    }
                    _ => {}
                }
            }
        }
    }
}
