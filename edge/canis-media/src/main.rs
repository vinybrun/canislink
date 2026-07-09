//! Lab WebRTC media plane: real ICE + DTLS peer connection + `canis-portal` datachannel.
//!
//! This is intentionally not a full camera pipeline yet. It *is* a real WebRTC session
//! between two processes — the missing piece for "media plane works" without claiming
//! production dog video.

use anyhow::{bail, Context};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use media_signal::SignalMsg;
use protocol::{DogId, SessionId};
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};
use uuid::Uuid;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

#[derive(Parser, Debug)]
#[command(name = "canis-media")]
struct Args {
    /// Signaling base, e.g. ws://127.0.0.1:8081
    #[arg(long, default_value = "ws://127.0.0.1:8081")]
    signal: String,
    #[arg(long)]
    session: Uuid,
    #[arg(long)]
    dog: Uuid,
    /// offerer | answerer
    #[arg(long)]
    role: String,
    #[arg(long, default_value_t = 45)]
    timeout_sec: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "canis_media=info,webrtc=warn".into()),
        )
        .init();

    let args = Args::parse();
    if args.role != "offerer" && args.role != "answerer" {
        bail!("role must be offerer or answerer");
    }
    let session = SessionId(args.session);
    let dog = DogId(args.dog);
    let offerer = args.role == "offerer";

    let pc = new_peer().await?;
    let (connected_tx, connected_rx) = tokio::sync::oneshot::channel::<()>();
    let connected_tx = Arc::new(tokio::sync::Mutex::new(Some(connected_tx)));
    {
        let connected_tx = connected_tx.clone();
        pc.on_peer_connection_state_change(Box::new(move |s| {
            let connected_tx = connected_tx.clone();
            Box::pin(async move {
                info!(?s, "peer connection state");
                if s == RTCPeerConnectionState::Connected {
                    if let Some(tx) = connected_tx.lock().await.take() {
                        let _ = tx.send(());
                    }
                }
            })
        }));
    }

    let portal_ok = Arc::new(Notify::new());
    if offerer {
        let dc = pc
            .create_data_channel(
                "canis-portal",
                Some(RTCDataChannelInit {
                    ordered: Some(true),
                    ..Default::default()
                }),
            )
            .await?;
        attach_dc(dc, portal_ok.clone());
    } else {
        let portal_ok2 = portal_ok.clone();
        pc.on_data_channel(Box::new(move |dc| {
            let portal_ok2 = portal_ok2.clone();
            Box::pin(async move {
                attach_dc(dc, portal_ok2);
            })
        }));
    }

    let (sig_out_tx, mut sig_out_rx) = mpsc::unbounded_channel::<String>();
    {
        let sig_out_tx = sig_out_tx.clone();
        let session = session;
        let dog = dog;
        pc.on_ice_candidate(Box::new(move |c| {
            let sig_out_tx = sig_out_tx.clone();
            Box::pin(async move {
                let Some(c) = c else { return };
                if let Ok(j) = c.to_json() {
                    let msg = SignalMsg::Ice {
                        session_id: session,
                        from: dog,
                        candidate: j.candidate,
                        sdp_mid: j.sdp_mid,
                        sdp_mline_index: j.sdp_mline_index.map(|x| x as u16),
                    };
                    if let Ok(s) = serde_json::to_string(&msg) {
                        let _ = sig_out_tx.send(s);
                    }
                }
            })
        }));
    }

    let ws_url = format!(
        "{}/v1/signal/{}",
        args.signal.trim_end_matches('/'),
        session
    );
    let (ws, _) = connect_async(&ws_url)
        .await
        .with_context(|| format!("connect {ws_url}"))?;
    let (mut sink, mut stream) = ws.split();

    // join room
    sink.send(Message::Text(
        serde_json::to_string(&SignalMsg::Join {
            session_id: session,
            dog_id: dog,
            role: args.role.clone(),
        })?
        .into(),
    ))
    .await?;

    // outbound signal writer
    let writer = tokio::spawn(async move {
        while let Some(text) = sig_out_rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    if offerer {
        // slight delay so answerer can join room first in lab scripts
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let offer = pc.create_offer(None).await?;
        pc.set_local_description(offer.clone()).await?;
        sig_out_tx.send(serde_json::to_string(&SignalMsg::Offer {
            session_id: session,
            from: dog,
            sdp: offer.sdp,
        })?)?;
        info!("sent offer");
    }

    let pc_in = pc.clone();
    let dog_in = dog;
    let session_in = session;
    let sig_out_tx2 = sig_out_tx.clone();
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            let Message::Text(text) = msg else { continue };
            let Ok(sig) = serde_json::from_str::<SignalMsg>(&text) else {
                continue;
            };
            if let Err(e) = handle_signal(&pc_in, dog_in, session_in, &sig_out_tx2, sig).await {
                warn!(error = %e, "signal handle");
            }
        }
    });

    info!(role = %args.role, "waiting for Connected…");
    tokio::select! {
        r = connected_rx => { r.ok(); info!("WebRTC Connected"); }
        _ = tokio::time::sleep(std::time::Duration::from_secs(args.timeout_sec)) => {
            bail!("timeout waiting for WebRTC Connected");
        }
    }

    tokio::select! {
        _ = portal_ok.notified() => info!("portal datachannel hello received — media plane OK"),
        _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
            warn!("no portal hello observed (ICE may work without message order)");
        }
    }

    writer.abort();
    reader.abort();
    pc.close().await?;
    info!("canis-media exit ok");
    Ok(())
}

async fn new_peer() -> anyhow::Result<Arc<RTCPeerConnection>> {
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };
    Ok(Arc::new(api.new_peer_connection(config).await?))
}

async fn handle_signal(
    pc: &Arc<RTCPeerConnection>,
    me: DogId,
    session: SessionId,
    out: &mpsc::UnboundedSender<String>,
    sig: SignalMsg,
) -> anyhow::Result<()> {
    match sig {
        SignalMsg::Offer { from, sdp, .. } if from != me => {
            let offer = RTCSessionDescription::offer(sdp)?;
            pc.set_remote_description(offer).await?;
            let answer = pc.create_answer(None).await?;
            pc.set_local_description(answer.clone()).await?;
            out.send(serde_json::to_string(&SignalMsg::Answer {
                session_id: session,
                from: me,
                sdp: answer.sdp,
            })?)?;
            info!("sent answer");
        }
        SignalMsg::Answer { from, sdp, .. } if from != me => {
            let answer = RTCSessionDescription::answer(sdp)?;
            pc.set_remote_description(answer).await?;
            info!("applied remote answer");
        }
        SignalMsg::Ice {
            from,
            candidate,
            sdp_mid,
            sdp_mline_index,
            ..
        } if from != me => {
            pc.add_ice_candidate(RTCIceCandidateInit {
                candidate,
                sdp_mid,
                sdp_mline_index: sdp_mline_index.map(|v| v as u16),
                ..Default::default()
            })
            .await?;
        }
        _ => {}
    }
    Ok(())
}

fn attach_dc(dc: Arc<RTCDataChannel>, portal_ok: Arc<Notify>) {
    let label = dc.label().to_string();
    info!(%label, "datachannel attached");
    let portal_ok2 = portal_ok.clone();
    dc.on_message(Box::new(move |msg| {
        let portal_ok2 = portal_ok2.clone();
        Box::pin(async move {
            let text = String::from_utf8_lossy(&msg.data);
            info!(%text, "portal rx");
            if text.contains("canis-portal") {
                portal_ok2.notify_waiters();
            }
        })
    }));
    let dc2 = dc.clone();
    dc.on_open(Box::new(move || {
        let dc2 = dc2.clone();
        Box::pin(async move {
            info!("datachannel open");
            let _ = dc2.send_text("canis-portal-hello".to_string()).await;
        })
    }));
}
