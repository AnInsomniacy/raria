use std::cmp;
use std::fmt;
use std::io;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use bytes::BufMut;
use crypto_bigint::modular::ConstMontyForm;
use crypto_bigint::{Encoding, U768, const_monty_params};
use rc4::consts::U20;
use rc4::{KeyInit, Rc4, StreamCipher};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf};

use crate::peer_connection::{PeerEncryptionMinLevel, PeerEncryptionMode, PeerEncryptionPolicy};

pub(crate) const BT_PROTOCOL_PREFIX: &[u8; 20] = b"\x13BitTorrent protocol";

const MODE_PLAINTEXT: u32 = 1;
const MODE_RC4: u32 = 2;
const MODE_ANY: u32 = MODE_PLAINTEXT | MODE_RC4;
const MAX_PADDING_LEN: usize = 512;
const VC_LEN: usize = 8;

const_monty_params!(
    DhPrime,
    U768,
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A63A36210000000000090563"
);

pub(crate) struct Encryptor(Rc4<U20>);

impl fmt::Debug for Encryptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Encryptor")
    }
}

impl Encryptor {
    pub(crate) fn encrypt(&mut self, buf: &mut [u8]) {
        self.0.apply_keystream(buf);
    }
}

pub(crate) struct Decryptor(Rc4<U20>);

impl fmt::Debug for Decryptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Decryptor")
    }
}

impl Decryptor {
    pub(crate) fn decrypt(&mut self, buf: &mut [u8]) {
        self.0.apply_keystream(buf);
    }
}

#[derive(Debug)]
pub(crate) struct Crypto {
    pub(crate) encryptor: Encryptor,
    pub(crate) decryptor: Decryptor,
}

pub(crate) struct DecryptingReader<R> {
    inner: R,
    decryptor: Decryptor,
}

pub(crate) struct BorrowingDecryptingReader<'a, R> {
    inner: R,
    decryptor: &'a mut Decryptor,
}

impl<'a, R> BorrowingDecryptingReader<'a, R> {
    pub(crate) fn new(inner: R, decryptor: &'a mut Decryptor) -> Self {
        Self { inner, decryptor }
    }
}

impl<R> DecryptingReader<R> {
    pub(crate) fn new(inner: R, decryptor: Decryptor) -> Self {
        Self { inner, decryptor }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for DecryptingReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let old_len = buf.filled().len();
        ready!(Pin::new(&mut self.inner).poll_read(cx, buf))?;
        let new_len = buf.filled().len();
        if new_len > old_len {
            self.decryptor.decrypt(&mut buf.filled_mut()[old_len..new_len]);
        }
        Poll::Ready(Ok(()))
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for BorrowingDecryptingReader<'_, R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let old_len = buf.filled().len();
        ready!(Pin::new(&mut self.inner).poll_read(cx, buf))?;
        let new_len = buf.filled().len();
        if new_len > old_len {
            self.decryptor.decrypt(&mut buf.filled_mut()[old_len..new_len]);
        }
        Poll::Ready(Ok(()))
    }
}

pub(crate) fn plaintext_incoming_allowed(policy: PeerEncryptionPolicy) -> bool {
    !matches!(policy.mode, PeerEncryptionMode::Require)
}

pub(crate) fn should_attempt_outbound_encryption(policy: PeerEncryptionPolicy) -> bool {
    !matches!(policy.mode, PeerEncryptionMode::Disabled)
}

pub(crate) fn requires_encrypted_transport(policy: PeerEncryptionPolicy) -> bool {
    matches!(policy.mode, PeerEncryptionMode::Require)
}

fn plaintext_after_pe_allowed(policy: PeerEncryptionPolicy) -> bool {
    !requires_encrypted_transport(policy)
        && matches!(policy.min_crypto_level, PeerEncryptionMinLevel::Plain)
}

pub(crate) fn write_post_pe_payload<'a>(
    encryptor: Option<&mut Encryptor>,
    buf: &'a mut [u8],
) -> &'a [u8] {
    if let Some(encryptor) = encryptor {
        encryptor.encrypt(buf);
    }
    buf
}

fn random_local_secret() -> U768 {
    U768::from_be_bytes(rand::random::<[u8; U768::BYTES]>().into())
}

fn local_pubkey(local_secret: &U768) -> U768 {
    const TWO: U768 = U768::from_u8(2);
    ConstMontyForm::<DhPrime, _>::new(&TWO)
        .pow(local_secret)
        .retrieve()
}

fn shared_secret(local_secret: &U768, remote_pubkey: &U768) -> U768 {
    ConstMontyForm::<DhPrime, _>::new(remote_pubkey)
        .pow(local_secret)
        .retrieve()
}

struct DhKeyExchange {
    local_secret: U768,
    local_pubkey: U768,
}

impl Default for DhKeyExchange {
    fn default() -> Self {
        let local_secret = random_local_secret();
        let local_pubkey = local_pubkey(&local_secret);
        Self {
            local_secret,
            local_pubkey,
        }
    }
}

impl DhKeyExchange {
    const KEY_SIZE: usize = U768::BYTES;

    fn local_pubkey(&self) -> &U768 {
        &self.local_pubkey
    }

    fn into_shared_secret(self, remote_pubkey: &U768) -> U768 {
        shared_secret(&self.local_secret, remote_pubkey)
    }
}

fn sha1_of(parts: &[&[u8]]) -> [u8; 20] {
    let mut hasher = sha1_smol::Sha1::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.digest().bytes()
}

fn xor_arrays<const N: usize>(lhs: [u8; N], rhs: [u8; N]) -> [u8; N] {
    let mut out = [0u8; N];
    for idx in 0..N {
        out[idx] = lhs[idx] ^ rhs[idx];
    }
    out
}

fn crypto_for_outbound_connection(
    secret: &U768,
    info_hash: &[u8; 20],
) -> io::Result<(Encryptor, Decryptor)> {
    let secret = secret.to_be_bytes();
    let encryption_key = sha1_of(&[b"keyA", &secret, info_hash]);
    let decryption_key = sha1_of(&[b"keyB", &secret, info_hash]);
    let mut encryptor = Encryptor(
        Rc4::new_from_slice(&encryption_key)
            .map_err(|e| io::Error::other(format!("rc4 outbound encryptor init failed: {e}")))?,
    );
    let mut decryptor = Decryptor(
        Rc4::new_from_slice(&decryption_key)
            .map_err(|e| io::Error::other(format!("rc4 outbound decryptor init failed: {e}")))?,
    );
    let mut discard = [0u8; 1024];
    encryptor.encrypt(&mut discard);
    decryptor.decrypt(&mut discard);
    Ok((encryptor, decryptor))
}

fn crypto_for_inbound_connection(
    secret: &U768,
    info_hash: &[u8; 20],
) -> io::Result<(Encryptor, Decryptor)> {
    let secret = secret.to_be_bytes();
    let encryption_key = sha1_of(&[b"keyB", &secret, info_hash]);
    let decryption_key = sha1_of(&[b"keyA", &secret, info_hash]);
    let mut encryptor = Encryptor(
        Rc4::new_from_slice(&encryption_key)
            .map_err(|e| io::Error::other(format!("rc4 inbound encryptor init failed: {e}")))?,
    );
    let mut decryptor = Decryptor(
        Rc4::new_from_slice(&decryption_key)
            .map_err(|e| io::Error::other(format!("rc4 inbound decryptor init failed: {e}")))?,
    );
    let mut discard = [0u8; 1024];
    encryptor.encrypt(&mut discard);
    decryptor.decrypt(&mut discard);
    Ok((encryptor, decryptor))
}

async fn read_remote_pubkey(
    stream: &mut (impl AsyncReadExt + Unpin),
    prefix: Option<&[u8]>,
) -> io::Result<U768> {
    let mut buf = [0u8; DhKeyExchange::KEY_SIZE];
    let prefix_len = prefix.map(|p| p.len()).unwrap_or(0);
    if let Some(prefix) = prefix {
        buf[..prefix.len()].copy_from_slice(prefix);
    }
    if prefix_len < buf.len() {
        stream.read_exact(&mut buf[prefix_len..]).await?;
    }
    Ok(U768::from_be_bytes(buf.into()))
}

async fn write_padding(stream: &mut (impl AsyncWriteExt + Unpin)) -> io::Result<()> {
    let padding: [u8; MAX_PADDING_LEN] = rand::random();
    let len = rand::random::<u16>() as usize % MAX_PADDING_LEN;
    stream.write_all(&padding[..len]).await
}

async fn consume_through<const N: usize>(
    source: &mut (impl AsyncReadExt + Unpin),
    pattern: &[u8; N],
) -> io::Result<()> {
    let mut storage = [MaybeUninit::<u8>::uninit(); N];
    let mut buf = ReadBuf::uninit(&mut storage);
    let mut overlap_ind = None;

    loop {
        let max_to_read = overlap_ind.unwrap_or(N);
        let bytes_read = source.read_buf(&mut buf.take(max_to_read)).await?;
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "stream exhausted before pattern found",
            ));
        }
        unsafe { buf.advance_mut(bytes_read) };

        if let Some(last_n) = buf.filled().last_chunk::<N>() {
            overlap_ind = overlap_start_index(pattern, last_n);
            match overlap_ind {
                Some(0) => return Ok(()),
                None => buf.clear(),
                Some(idx) => {
                    buf.filled_mut().copy_within(idx.., 0);
                    buf.set_filled(N - idx);
                }
            }
        }
    }
}

fn overlap_start_index<const N: usize>(pattern: &[u8; N], data: &[u8; N]) -> Option<usize> {
    let mut data_idx = 0;
    let mut pattern_idx = 0;
    let mut result = None;

    while data_idx < N {
        if data[data_idx] == pattern[pattern_idx] {
            result.get_or_insert(data_idx);
            data_idx += 1;
            pattern_idx += 1;
        } else {
            if let Some(old_result) = result.take() {
                data_idx = old_result;
                pattern_idx = 0;
            }
            data_idx += 1;
        }
    }

    result
}

async fn consume_encrypted(
    stream: &mut (impl AsyncReadExt + Unpin),
    len: usize,
    decryptor: &mut Decryptor,
    what: &'static str,
) -> io::Result<()> {
    let mut remaining = len;
    let mut buf = [0u8; MAX_PADDING_LEN];
    while remaining > 0 {
        let to_read = cmp::min(remaining, buf.len());
        stream.read_exact(&mut buf[..to_read]).await?;
        decryptor.decrypt(&mut buf[..to_read]);
        remaining -= to_read;
    }
    if len > MAX_PADDING_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{what} length {len} exceeds {MAX_PADDING_LEN}"),
        ));
    }
    Ok(())
}

fn max_pe3_len() -> usize {
    20 + 20 + VC_LEN + 4 + 2 + MAX_PADDING_LEN + 2
}

fn max_pe4_len() -> usize {
    VC_LEN + 4 + 2 + MAX_PADDING_LEN
}

pub(crate) async fn negotiate_outbound(
    stream: &mut tokio::net::TcpStream,
    info_hash: &[u8; 20],
    policy: PeerEncryptionPolicy,
) -> io::Result<Option<Crypto>> {
    if !should_attempt_outbound_encryption(policy) {
        return Ok(None);
    }

    let dh = DhKeyExchange::default();
    stream.write_all(&dh.local_pubkey().to_be_bytes()).await?;
    let remote_pubkey = read_remote_pubkey(stream, None).await?;
    let secret = dh.into_shared_secret(&remote_pubkey);
    let secret_bytes = secret.to_be_bytes();
    let (mut encryptor, mut decryptor) = crypto_for_outbound_connection(&secret, info_hash)?;

    let (mut ingress, mut egress) = stream.split();

    let write_pe3 = async {
        write_padding(&mut egress).await?;

        let mut buf = [0u8; 20 + 20 + VC_LEN + 4 + 2 + MAX_PADDING_LEN + 2];
        let mut writer = &mut buf[..];
        let hash_req1_s = sha1_of(&[b"req1", &secret_bytes]);
        let hash_req2_req3_xored = xor_arrays(
            sha1_of(&[b"req2", info_hash]),
            sha1_of(&[b"req3", &secret_bytes]),
        );
        let padding_c_len = rand::random::<u16>() as usize % MAX_PADDING_LEN;

        writer[..20].copy_from_slice(&hash_req1_s);
        writer = &mut writer[20..];
        writer[..20].copy_from_slice(&hash_req2_req3_xored);
        writer = &mut writer[20..];
        writer[..VC_LEN].fill(0);
        writer = &mut writer[VC_LEN..];
        writer[..4].copy_from_slice(&MODE_ANY.to_be_bytes());
        writer = &mut writer[4..];
        writer[..2].copy_from_slice(&(padding_c_len as u16).to_be_bytes());
        writer = &mut writer[2..];
        writer[..padding_c_len].fill(0);
        writer = &mut writer[padding_c_len..];
        writer[..2].copy_from_slice(&0u16.to_be_bytes());

        let total_len = max_pe3_len() - MAX_PADDING_LEN + padding_c_len;
        encryptor.encrypt(&mut buf[40..total_len]);
        egress.write_all(&buf[..total_len]).await
    };

    let mut crypto_select = 0u32;
    let read_pe4 = async {
        let mut expected_remote_vc = [0u8; VC_LEN];
        decryptor.decrypt(&mut expected_remote_vc);
        consume_through(&mut ingress, &expected_remote_vc).await?;

        let mut buf = [0u8; 6];
        ingress.read_exact(&mut buf).await?;
        decryptor.decrypt(&mut buf);
        crypto_select = u32::from_be_bytes(buf[..4].try_into().unwrap());
        let padding_d_len = u16::from_be_bytes(buf[4..6].try_into().unwrap()) as usize;
        if padding_d_len > MAX_PADDING_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("padding D len {padding_d_len} exceeds {MAX_PADDING_LEN}"),
            ));
        }
        consume_encrypted(&mut ingress, padding_d_len, &mut decryptor, "padding D").await
    };

    tokio::try_join!(write_pe3, read_pe4)?;

    match crypto_select {
        MODE_RC4 => Ok(Some(Crypto {
            encryptor,
            decryptor,
        })),
        MODE_PLAINTEXT if plaintext_after_pe_allowed(policy) => Ok(None),
        MODE_PLAINTEXT => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "remote peer selected plaintext while encrypted transport is required",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected crypto select {crypto_select:#x}"),
        )),
    }
}

pub(crate) async fn negotiate_incoming(
    stream: &mut tokio::net::TcpStream,
    first_prefix: &[u8],
    candidate_info_hashes: &[[u8; 20]],
    policy: PeerEncryptionPolicy,
) -> io::Result<([u8; 20], Option<Crypto>)> {
    let dh = DhKeyExchange::default();
    let remote_pubkey = read_remote_pubkey(stream, Some(first_prefix)).await?;

    stream.write_all(&dh.local_pubkey().to_be_bytes()).await?;
    write_padding(stream).await?;

    let secret = dh.into_shared_secret(&remote_pubkey);
    let secret_bytes = secret.to_be_bytes();
    let expected_hash_req1_s = sha1_of(&[b"req1", &secret_bytes]);
    consume_through(stream, &expected_hash_req1_s).await?;

    let mut hash_req2_req3_xored = [0u8; 20];
    stream.read_exact(&mut hash_req2_req3_xored).await?;

    let matched_info_hash = candidate_info_hashes
        .iter()
        .copied()
        .find(|info_hash| {
            hash_req2_req3_xored
                == xor_arrays(
                    sha1_of(&[b"req2", info_hash]),
                    sha1_of(&[b"req3", &secret_bytes]),
                )
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no matching torrent for encrypted peer"))?;

    let (mut encryptor, mut decryptor) = crypto_for_inbound_connection(&secret, &matched_info_hash)?;

    let mut buf = [0u8; VC_LEN + 4 + 2];
    stream.read_exact(&mut buf).await?;
    decryptor.decrypt(&mut buf);

    let remote_vc = u64::from_be_bytes(buf[..VC_LEN].try_into().unwrap());
    if remote_vc != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected non-zero VC from remote peer",
        ));
    }

    let crypto_provide = u32::from_be_bytes(buf[VC_LEN..VC_LEN + 4].try_into().unwrap());
    if crypto_provide & MODE_ANY == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unexpected crypto provide {crypto_provide:#x}"),
        ));
    }
    let padding_c_len =
        u16::from_be_bytes(buf[VC_LEN + 4..VC_LEN + 6].try_into().unwrap()) as usize;
    if padding_c_len > MAX_PADDING_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("padding C len {padding_c_len} exceeds {MAX_PADDING_LEN}"),
        ));
    }
    consume_encrypted(stream, padding_c_len, &mut decryptor, "padding C").await?;

    let mut ia_len_bytes = [0u8; 2];
    stream.read_exact(&mut ia_len_bytes).await?;
    decryptor.decrypt(&mut ia_len_bytes);
    let ia_len = u16::from_be_bytes(ia_len_bytes) as usize;
    if ia_len != 0 {
        consume_encrypted(stream, ia_len, &mut decryptor, "IA").await?;
    }

    let crypto_select = if crypto_provide & MODE_RC4 != 0 {
        MODE_RC4
    } else {
        MODE_PLAINTEXT
    };
    let padding_d_len = rand::random::<u16>() as usize % MAX_PADDING_LEN;

    let mut pe4 = [0u8; VC_LEN + 4 + 2 + MAX_PADDING_LEN];
    pe4[..VC_LEN].fill(0);
    pe4[VC_LEN..VC_LEN + 4].copy_from_slice(&crypto_select.to_be_bytes());
    pe4[VC_LEN + 4..VC_LEN + 6].copy_from_slice(&(padding_d_len as u16).to_be_bytes());
    encryptor.encrypt(&mut pe4[..max_pe4_len() - MAX_PADDING_LEN + padding_d_len]);
    stream
        .write_all(&pe4[..max_pe4_len() - MAX_PADDING_LEN + padding_d_len])
        .await?;

    match crypto_select {
        MODE_RC4 => Ok((
            matched_info_hash,
            Some(Crypto {
                encryptor,
                decryptor,
            }),
        )),
        MODE_PLAINTEXT if plaintext_after_pe_allowed(policy) => {
            Ok((matched_info_hash, None))
        }
        MODE_PLAINTEXT => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "incoming peer does not support encrypted transport",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected crypto select {crypto_select:#x}"),
        )),
    }
}
