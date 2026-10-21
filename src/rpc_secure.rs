use crate::{rpc_protocol::{RpcConnectionAdapter, RpcError}, types::Id};
use snow::{Builder, HandshakeState, TransportState};
use std::sync::{Arc, Mutex};

const PROTOCOL: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
const PROLOGUE: &[u8] = b"flux.rpc.secure.v1";
const HANDSHAKE_LIMIT: usize = 128;
const TAG_BYTES: usize = 16;
const MAX_MESSAGE: usize = 65535;

pub enum HandshakeRole {
    Initiator,
    Responder,
}

pub struct RpcHandshake {
    state: Option<HandshakeState>,
    max_message_bytes: usize,
    datagrams: bool,
}

impl RpcHandshake {
    pub fn public_key(private_key: &[u8; 32]) -> Result<[u8; 32], RpcError> {
        use snow::resolvers::{CryptoResolver, DefaultResolver};
        if private_key == &[0; 32] { return Err(RpcError::InvalidPayload); }
        let params: snow::params::NoiseParams = PROTOCOL.parse().map_err(|_| RpcError::HandlerFailed)?;
        let mut dh = DefaultResolver.resolve_dh(&params.dh).ok_or(RpcError::HandlerFailed)?;
        dh.set(private_key);
        dh.pubkey().try_into().map_err(|_| RpcError::HandlerFailed)
    }

    pub fn new(role: HandshakeRole, private_key: &[u8; 32], max_message_bytes: usize) -> Result<Self, RpcError> {
        Self::build(role, private_key, max_message_bytes, false)
    }

    pub fn new_datagrams(role: HandshakeRole, private_key: &[u8; 32], max_message_bytes: usize) -> Result<Self, RpcError> {
        Self::build(role, private_key, max_message_bytes, true)
    }

    fn build(role: HandshakeRole, private_key: &[u8; 32], max_message_bytes: usize, datagrams: bool) -> Result<Self, RpcError> {
        if !(HANDSHAKE_LIMIT..=MAX_MESSAGE).contains(&max_message_bytes) || private_key == &[0; 32] {
            return Err(RpcError::InvalidPayload);
        }
        let builder = Builder::new(PROTOCOL.parse().map_err(|_| RpcError::HandlerFailed)?)
            .local_private_key(private_key).map_err(|_| RpcError::HandlerFailed)?
            .prologue(if datagrams { b"flux.rpc.secure.datagram.v1" } else { PROLOGUE }).map_err(|_| RpcError::HandlerFailed)?;
        let state = match role {
            HandshakeRole::Initiator => builder.build_initiator(),
            HandshakeRole::Responder => builder.build_responder(),
        }.map_err(|_| RpcError::HandlerFailed)?;
        Ok(Self { state: Some(state), max_message_bytes, datagrams })
    }

    pub fn write(&mut self) -> Result<Vec<u8>, RpcError> {
        let mut state = self.state.take().ok_or(RpcError::Disconnected)?;
        let mut output = vec![0; HANDSHAKE_LIMIT];
        let length = state.write_message(&[], &mut output).map_err(|_| RpcError::Unauthenticated)?;
        output.truncate(length);
        self.state = Some(state);
        Ok(output)
    }

    pub fn read(&mut self, message: &[u8]) -> Result<(), RpcError> {
        let mut state = self.state.take().ok_or(RpcError::Disconnected)?;
        if message.is_empty() || message.len() > HANDSHAKE_LIMIT { return Err(RpcError::InvalidPayload); }
        let mut output = [0; HANDSHAKE_LIMIT];
        let length = state.read_message(message, &mut output).map_err(|_| RpcError::Unauthenticated)?;
        if length != 0 { return Err(RpcError::InvalidPayload); }
        self.state = Some(state);
        Ok(())
    }

    pub fn peer_identity(&self) -> Result<([u8; 32], [u8; 32]), RpcError> {
        let state = self.state.as_ref().ok_or(RpcError::Disconnected)?;
        if !state.is_handshake_finished() { return Err(RpcError::Unauthenticated); }
        let key = state.get_remote_static().ok_or(RpcError::Unauthenticated)?
            .try_into().map_err(|_| RpcError::Unauthenticated)?;
        let binding = state.get_handshake_hash().try_into().map_err(|_| RpcError::Unauthenticated)?;
        Ok((key, binding))
    }

    pub fn approve(self, expected_peer_key: &[u8; 32]) -> Result<SecureRpcChannel, RpcError> {
        if self.datagrams { return Err(RpcError::InvalidPayload); }
        let (peer_key, binding) = self.peer_identity()?;
        if &peer_key != expected_peer_key { return Err(RpcError::Unauthenticated); }
        let state = self.state.ok_or(RpcError::Disconnected)?.into_transport_mode()
            .map_err(|_| RpcError::Unauthenticated)?;
        Ok(SecureRpcChannel { state: Some(state), peer_key, binding, max_message_bytes: self.max_message_bytes })
    }

    pub fn approve_datagrams(self, expected_peer_key: &[u8; 32]) -> Result<SecureDatagramChannel, RpcError> {
        if !self.datagrams { return Err(RpcError::InvalidPayload); }
        let (peer_key, _) = self.peer_identity()?;
        if &peer_key != expected_peer_key { return Err(RpcError::Unauthenticated); }
        let state = self.state.ok_or(RpcError::Disconnected)?.into_stateless_transport_mode()
            .map_err(|_| RpcError::Unauthenticated)?;
        Ok(SecureDatagramChannel {
            state: Some(state), outgoing: [0; 2], incoming: [None; 2],
            max_message_bytes: self.max_message_bytes,
        })
    }
}

pub struct SecureDatagramChannel {
    state: Option<snow::StatelessTransportState>,
    outgoing: [u64; 2],
    incoming: [Option<u64>; 2],
    max_message_bytes: usize,
}

impl SecureDatagramChannel {
    pub fn close(&mut self) { self.state = None; }

    pub fn encrypt(&mut self, lane: u8, plaintext: &[u8]) -> Result<Vec<u8>, RpcError> {
        let index = usize::from(lane);
        if index >= 2 || plaintext.is_empty() || plaintext.len() + TAG_BYTES + 8 > self.max_message_bytes {
            return Err(RpcError::InvalidPayload);
        }
        let sequence = self.outgoing[index];
        if sequence >= u64::MAX / 2 {
            self.close();
            return Err(RpcError::Disconnected);
        }
        let nonce = sequence * 2 + u64::from(lane);
        self.outgoing[index] += 1;
        let state = self.state.as_ref().ok_or(RpcError::Disconnected)?;
        let mut output = vec![0; plaintext.len() + TAG_BYTES + 8];
        output[..8].copy_from_slice(&nonce.to_be_bytes());
        state.write_message(nonce, plaintext, &mut output[8..]).map_err(|_| RpcError::HandlerFailed)?;
        Ok(output)
    }

    pub fn decrypt(&mut self, lane: u8, packet: &[u8]) -> Result<Vec<u8>, RpcError> {
        let index = usize::from(lane);
        if index >= 2 || packet.len() <= TAG_BYTES + 8 || packet.len() > self.max_message_bytes {
            return Err(RpcError::InvalidPayload);
        }
        let nonce = u64::from_be_bytes(packet[..8].try_into().map_err(|_| RpcError::InvalidPayload)?);
        let sequence = nonce / 2;
        if nonce % 2 != u64::from(lane) || self.incoming[index].is_some_and(|last| sequence <= last) {
            return Err(RpcError::Unauthenticated);
        }
        let state = self.state.as_ref().ok_or(RpcError::Disconnected)?;
        let mut output = vec![0; packet.len() - 8 - TAG_BYTES];
        let length = state.read_message(nonce, &packet[8..], &mut output)
            .map_err(|_| RpcError::Unauthenticated)?;
        output.truncate(length);
        self.incoming[index] = Some(sequence);
        Ok(output)
    }
}

pub struct SecureRpcChannel {
    state: Option<TransportState>,
    peer_key: [u8; 32],
    binding: [u8; 32],
    max_message_bytes: usize,
}

pub struct SecureRpcAdapter {
    transport: Arc<dyn RpcConnectionAdapter>,
    channel: Mutex<SecureRpcChannel>,
    max_plaintext_bytes: usize,
}

impl SecureRpcAdapter {
    pub fn new(transport: Arc<dyn RpcConnectionAdapter>, channel: SecureRpcChannel) -> Result<Self, RpcError> {
        if transport.max_message_bytes() < channel.max_message_bytes { return Err(RpcError::InvalidPayload); }
        Ok(Self { max_plaintext_bytes: channel.max_plaintext_bytes(), transport, channel: Mutex::new(channel) })
    }

    pub fn receive(&self, ciphertext: &[u8]) -> Result<Vec<u8>, RpcError> {
        let mut channel = self.channel.lock().unwrap_or_else(|error| error.into_inner());
        match channel.decrypt(ciphertext) {
            Ok(plaintext) => Ok(plaintext),
            Err(error) => {
                channel.close();
                self.transport.close();
                Err(error)
            }
        }
    }
}

impl RpcConnectionAdapter for SecureRpcAdapter {
    fn authenticated_peer(&self) -> Id { self.transport.authenticated_peer() }
    fn max_message_bytes(&self) -> usize { self.max_plaintext_bytes }

    fn send(&self, frame: Vec<u8>) -> Result<(), RpcError> {
        if frame.is_empty() || frame.len() > self.max_plaintext_bytes { return Err(RpcError::InvalidPayload); }
        let mut channel = self.channel.lock().unwrap_or_else(|error| error.into_inner());
        let result = channel.encrypt(&frame).and_then(|ciphertext| self.transport.send(ciphertext));
        if result.is_err() {
            channel.close();
            self.transport.close();
        }
        result
    }

    fn close(&self) {
        self.channel.lock().unwrap_or_else(|error| error.into_inner()).close();
        self.transport.close();
    }
}

impl SecureRpcChannel {
    pub fn peer_key(&self) -> &[u8; 32] { &self.peer_key }
    pub fn binding(&self) -> &[u8; 32] { &self.binding }
    pub fn max_plaintext_bytes(&self) -> usize { self.max_message_bytes - TAG_BYTES }
    pub fn close(&mut self) { self.state = None; }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, RpcError> {
        if plaintext.is_empty() || plaintext.len() > self.max_plaintext_bytes() { return Err(RpcError::InvalidPayload); }
        let mut state = self.state.take().ok_or(RpcError::Disconnected)?;
        let mut output = vec![0; plaintext.len() + TAG_BYTES];
        let length = state.write_message(plaintext, &mut output).map_err(|_| RpcError::Unauthenticated)?;
        output.truncate(length);
        self.state = Some(state);
        Ok(output)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, RpcError> {
        let mut state = self.state.take().ok_or(RpcError::Disconnected)?;
        if ciphertext.len() <= TAG_BYTES || ciphertext.len() > self.max_message_bytes { return Err(RpcError::InvalidPayload); }
        let mut output = vec![0; ciphertext.len() - TAG_BYTES];
        let length = state.read_message(ciphertext, &mut output).map_err(|_| RpcError::Unauthenticated)?;
        output.truncate(length);
        self.state = Some(state);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshakes() -> (RpcHandshake, RpcHandshake, [u8; 32], [u8; 32]) {
        let builder = Builder::new(PROTOCOL.parse().unwrap());
        let client_key = builder.generate_keypair().unwrap();
        let server_key = builder.generate_keypair().unwrap();
        let mut client = RpcHandshake::new(HandshakeRole::Initiator, client_key.private.as_slice().try_into().unwrap(), 1024).unwrap();
        let mut server = RpcHandshake::new(HandshakeRole::Responder, server_key.private.as_slice().try_into().unwrap(), 1024).unwrap();
        assert!(client.peer_identity().is_err());
        server.read(&client.write().unwrap()).unwrap();
        client.read(&server.write().unwrap()).unwrap();
        server.read(&client.write().unwrap()).unwrap();
        assert_eq!(client.peer_identity().unwrap().1, server.peer_identity().unwrap().1);
        (client, server, client_key.public.as_slice().try_into().unwrap(), server_key.public.as_slice().try_into().unwrap())
    }

    fn connected() -> (SecureRpcChannel, SecureRpcChannel) {
        let (client, server, client_key, server_key) = handshakes();
        (client.approve(&server_key).unwrap(), server.approve(&client_key).unwrap())
    }

    #[test]
    fn untrusted_keys_and_cross_connection_ciphertext_are_rejected() {
        let (client, _, _, mut server_key) = handshakes();
        server_key[0] ^= 1;
        assert!(matches!(client.approve(&server_key), Err(RpcError::Unauthenticated)));
        let (mut first, _) = connected();
        let (_, mut second_server) = connected();
        assert_ne!(first.binding(), second_server.binding());
        assert!(second_server.decrypt(&first.encrypt(b"request").unwrap()).is_err());
        assert_eq!(second_server.encrypt(b"reply"), Err(RpcError::Disconnected));
    }

    #[test]
    fn encrypted_messages_are_bidirectional_and_replays_close_connection() {
        let (mut client, mut server) = connected();
        let message = client.encrypt(b"request").unwrap();
        assert_eq!(server.decrypt(&message).unwrap(), b"request");
        assert_eq!(client.decrypt(&server.encrypt(b"reply").unwrap()).unwrap(), b"reply");
        assert!(server.decrypt(&message).is_err());
        assert_eq!(server.decrypt(&client.encrypt(b"next").unwrap()), Err(RpcError::Disconnected));
    }

    #[test]
    fn modified_ciphertext_is_rejected_and_plaintext_is_bounded() {
        let (mut client, mut server) = connected();
        assert!(client.encrypt(&vec![0; 1009]).is_err());
        let mut message = client.encrypt(b"request").unwrap();
        message[0] ^= 1;
        assert!(server.decrypt(&message).is_err());
        assert_eq!(server.encrypt(b"reply"), Err(RpcError::Disconnected));
        client.close();
        assert_eq!(client.encrypt(b"request"), Err(RpcError::Disconnected));
    }

    #[test]
    fn secure_adapter_wraps_transport_and_retires_dropped_ciphertext() {
        use crate::rpc_protocol::QueuedRpcAdapter;
        let (client, server) = connected();
        let outgoing = Arc::new(QueuedRpcAdapter::new(Id::new(), 1024, 1).unwrap());
        let incoming = Arc::new(QueuedRpcAdapter::new(Id::new(), 1024, 1).unwrap());
        let client = SecureRpcAdapter::new(outgoing.clone(), client).unwrap();
        let server = SecureRpcAdapter::new(incoming.clone(), server).unwrap();
        assert_eq!(client.max_message_bytes(), 1008);
        client.send(b"request".to_vec()).unwrap();
        assert_eq!(server.receive(&outgoing.try_recv().unwrap().unwrap()).unwrap(), b"request");
        server.send(b"reply".to_vec()).unwrap();
        assert_eq!(client.receive(&incoming.try_recv().unwrap().unwrap()).unwrap(), b"reply");
        client.send(b"queued".to_vec()).unwrap();
        assert_eq!(client.send(b"overflow".to_vec()), Err(RpcError::Overloaded));
        assert_eq!(outgoing.try_recv(), Err(RpcError::Disconnected));
        assert_eq!(client.send(b"next".to_vec()), Err(RpcError::Disconnected));
    }
}