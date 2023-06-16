use core::task::{Context, Poll};
use futures_util::{ready, Future};
use hyper::server::{
    accept::Accept,
    conn::{AddrIncoming, AddrStream},
};
use rustls::{server::ResolvesServerCertUsingSni, sign::RsaSigningKey, ServerConfig};
use std::{io, pin::Pin, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

enum State {
    Handshaking(tokio_rustls::Accept<AddrStream>),
    Streaming(tokio_rustls::server::TlsStream<AddrStream>),
}
pub struct TlsStream {
    state: State,
}
impl TlsStream {
    fn new(stream: AddrStream, config: Arc<ServerConfig>) -> TlsStream {
        let accept = tokio_rustls::TlsAcceptor::from(config).accept(stream);
        TlsStream {
            state: State::Handshaking(accept),
        }
    }
}
impl AsyncRead for TlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let pin = self.get_mut();
        match pin.state {
            State::Handshaking(ref mut accept) => match ready!(Pin::new(accept).poll(cx)) {
                Ok(mut stream) => {
                    let result = Pin::new(&mut stream).poll_read(cx, buf);
                    pin.state = State::Streaming(stream);
                    result
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            State::Streaming(ref mut stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}
impl AsyncWrite for TlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let pin = self.get_mut();
        match pin.state {
            State::Handshaking(ref mut accept) => match ready!(Pin::new(accept).poll(cx)) {
                Ok(mut stream) => {
                    let result = Pin::new(&mut stream).poll_write(cx, buf);
                    pin.state = State::Streaming(stream);
                    result
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            State::Streaming(ref mut stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.state {
            State::Handshaking(_) => Poll::Ready(Ok(())),
            State::Streaming(ref mut stream) => Pin::new(stream).poll_flush(cx),
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.state {
            State::Handshaking(_) => Poll::Ready(Ok(())),
            State::Streaming(ref mut stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

pub struct TlsAcceptor {
    config: Arc<ServerConfig>,
    incoming: AddrIncoming,
}
impl TlsAcceptor {
    pub fn new(config: Arc<ServerConfig>, incoming: AddrIncoming) -> TlsAcceptor {
        TlsAcceptor { config, incoming }
    }
}
impl Accept for TlsAcceptor {
    type Conn = TlsStream;
    type Error = io::Error;

    fn poll_accept(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        let pin = self.get_mut();
        match ready!(Pin::new(&mut pin.incoming).poll_accept(cx)) {
            Some(Ok(sock)) => Poll::Ready(Some(Ok(TlsStream::new(sock, pin.config.clone())))),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Ready(None),
        }
    }
}

pub(super) fn error(err: String) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err)
}

pub(super) fn add_certificate_to_resolver(
    name: &str,
    hostname: &str,
    resolver: &mut ResolvesServerCertUsingSni,
) {
    resolver
        .add(
            hostname,
            rustls::sign::CertifiedKey::new(
                load_certs(&format!("certificates/{}/fullchain.pem", name)).unwrap(),
                Arc::new(
                    RsaSigningKey::new(
                        &load_private_key(&format!("certificates/{}/privkey.pem", name)).unwrap(),
                    )
                    .unwrap(),
                ),
            ),
        )
        .expect(&("Invalid certificate for ".to_owned() + hostname));
}

pub(super) fn load_certs(filename: &str) -> io::Result<Vec<rustls::Certificate>> {
    let certs = rustls_pemfile::certs(&mut io::BufReader::new(
        std::fs::File::open(filename)
            .map_err(|e| error(format!("failed to open {}: {}", filename, e)))?,
    ))
    .map_err(|_| error("failed to load certificate".into()))?;
    Ok(certs.into_iter().map(rustls::Certificate).collect())
}

pub(super) fn load_private_key(filename: &str) -> io::Result<rustls::PrivateKey> {
    let keys = rustls_pemfile::rsa_private_keys(&mut io::BufReader::new(
        std::fs::File::open(filename)
            .map_err(|e| error(format!("failed to open {}: {}", filename, e)))?,
    ))
    .map_err(|_| error("failed to load private key".into()))?;
    if keys.len() != 1 {
        return Err(error("expected a single private key".into()));
    }
    Ok(rustls::PrivateKey(keys[0].clone()))
}
