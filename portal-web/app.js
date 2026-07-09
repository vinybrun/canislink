/* CanisLink dog video portal — browser / Android WebView */
(function () {
  const $ = (id) => document.getElementById(id);
  const state = {
    api: localStorage.getItem("apiBase") || "http://10.0.2.2:18080",
    signal: localStorage.getItem("signalBase") || "ws://10.0.2.2:18081",
    token: localStorage.getItem("token") || "",
    terminalId: localStorage.getItem("terminalId") || "",
    dogId: localStorage.getItem("dogId") || "",
    seq: 0,
    ux: "idle",
    invite: null,
    session: null,
    role: null,
    pc: null,
    ws: null,
    localStream: null,
  };

  function log(msg) {
    const el = $("log");
    el.textContent = `[${new Date().toISOString().slice(11, 19)}] ${msg}\n` + el.textContent;
    console.log(msg);
  }
  function setUx(u) {
    state.ux = u;
    $("uxState").textContent = u;
  }
  function authHeaders() {
    return {
      "Content-Type": "application/json",
      Authorization: `Device ${state.terminalId}:${state.token}`,
    };
  }
  function loadForm() {
    $("apiBase").value = state.api;
    $("signalBase").value = state.signal;
    $("token").value = state.token;
    $("terminalId").value = state.terminalId;
    $("dogId").value = state.dogId;
  }
  function saveForm() {
    state.api = $("apiBase").value.trim().replace(/\/$/, "");
    state.signal = $("signalBase").value.trim().replace(/\/$/, "");
    state.token = $("token").value.trim();
    state.terminalId = $("terminalId").value.trim();
    state.dogId = $("dogId").value.trim();
    localStorage.setItem("apiBase", state.api);
    localStorage.setItem("signalBase", state.signal);
    localStorage.setItem("token", state.token);
    localStorage.setItem("terminalId", state.terminalId);
    localStorage.setItem("dogId", state.dogId);
    log("identity saved");
  }

  async function publishPresent() {
    state.seq += 1;
    const body = {
      dog_id: state.dogId,
      terminal_id: state.terminalId,
      present: true,
      confidence: 0.95,
      force_band: "medium",
      force_n: 120,
      tof_mm: 400,
      ts: new Date().toISOString(),
      seq: state.seq,
    };
    const r = await fetch(`${state.api}/v1/presence`, {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify(body),
    });
    if (!r.ok) throw new Error(`presence ${r.status}`);
    setUx("present");
    log("presence published (phone substitutes mat)");
  }

  async function callFriend() {
    await publishPresent();
    const r = await fetch(`${state.api}/v1/invites`, {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify({
        mode: "portal",
        to_dog: null,
        dog_id: state.dogId,
        terminal_id: state.terminalId,
      }),
    });
    const t = await r.text();
    if (!r.ok) throw new Error(`invite ${r.status} ${t}`);
    const data = JSON.parse(t);
    state.invite = data.invite;
    setUx("ringing_out");
    log(`ringing out invite=${data.invite.id}`);
    // poll active session after peer accepts
    pollSessionLoop();
  }

  async function pollIncoming() {
    const q = new URLSearchParams({
      dog_id: state.dogId,
      terminal_id: state.terminalId,
    });
    const r = await fetch(`${state.api}/v1/invites/incoming?${q}`, {
      headers: authHeaders(),
    });
    if (!r.ok) throw new Error(`incoming ${r.status}`);
    const offer = await r.json();
    if (offer && offer.invite) {
      state.invite = offer.invite;
      setUx("ringing_in");
      $("btnAccept").disabled = false;
      log(`LURE invite from ${offer.invite.from_dog}`);
      // dog-native: flash background
      document.body.style.outline = "4px solid #2f6fed";
      setTimeout(() => (document.body.style.outline = ""), 800);
    } else {
      log("no incoming");
    }
  }

  async function accept() {
    if (!state.invite) throw new Error("no invite");
    await publishPresent();
    const r = await fetch(`${state.api}/v1/invites/${state.invite.id}/accept`, {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify({
        dog_id: state.dogId,
        terminal_id: state.terminalId,
      }),
    });
    const t = await r.text();
    if (!r.ok) throw new Error(`accept ${r.status} ${t}`);
    const data = JSON.parse(t);
    state.session = data.session;
    state.role = data.webrtc_role;
    $("sessionId").textContent = data.session.id;
    setUx("negotiating");
    log(`accepted role=${data.webrtc_role}`);
    await mediaReady(true);
    await startWebRtc(data.webrtc_role === "offerer");
  }

  async function mediaReady(ready) {
    if (!state.session) return;
    const r = await fetch(
      `${state.api}/v1/sessions/${state.session.id}/media_ready`,
      {
        method: "POST",
        headers: authHeaders(),
        body: JSON.stringify({
          dog_id: state.dogId,
          terminal_id: state.terminalId,
          ready,
        }),
      }
    );
    if (!r.ok) throw new Error(`media_ready ${r.status}`);
    const data = await r.json();
    state.session = data.session;
    if (data.both_ready) setUx("in_session");
    log(`media_ready both=${data.both_ready}`);
  }

  async function pollSessionLoop() {
    for (let i = 0; i < 40; i++) {
      await new Promise((r) => setTimeout(r, 500));
      const q = new URLSearchParams({
        dog_id: state.dogId,
        terminal_id: state.terminalId,
      });
      const r = await fetch(`${state.api}/v1/sessions/active?${q}`, {
        headers: authHeaders(),
      });
      if (!r.ok) continue;
      const sess = await r.json();
      if (sess && sess.id) {
        state.session = sess;
        $("sessionId").textContent = sess.id;
        // caller is offerer per architecture
        state.role = "offerer";
        setUx("negotiating");
        await mediaReady(true);
        await startWebRtc(true);
        return;
      }
    }
    log("no session became active (peer may not have accepted)");
  }

  async function startWebRtc(isOfferer) {
    if (state.pc) {
      try { state.pc.close(); } catch (_) {}
    }
    const pc = new RTCPeerConnection({
      iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
    });
    state.pc = pc;

    // local camera/mic — phone IS the dog portal hardware
    try {
      state.localStream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: "environment" },
        audio: true,
      });
    } catch (e) {
      log("getUserMedia failed, trying video-only: " + e.message);
      state.localStream = await navigator.mediaDevices.getUserMedia({
        video: true,
        audio: false,
      });
    }
    $("localVideo").srcObject = state.localStream;
    for (const track of state.localStream.getTracks()) {
      pc.addTrack(track, state.localStream);
    }

    pc.ontrack = (ev) => {
      log("remote track " + ev.track.kind);
      $("remoteVideo").srcObject = ev.streams[0];
      setUx("in_session_video");
    };
    pc.onconnectionstatechange = () => log("pc " + pc.connectionState);

    const wsUrl = `${state.signal}/v1/signal/${state.session.id}`;
    const ws = new WebSocket(wsUrl);
    state.ws = ws;

    ws.onopen = async () => {
      log("signal open");
      ws.send(
        JSON.stringify({
          type: "join",
          session_id: state.session.id,
          dog_id: state.dogId,
          role: isOfferer ? "offerer" : "answerer",
        })
      );
      if (isOfferer) {
        const offer = await pc.createOffer();
        await pc.setLocalDescription(offer);
        ws.send(
          JSON.stringify({
            type: "offer",
            session_id: state.session.id,
            from: state.dogId,
            sdp: offer.sdp,
          })
        );
        log("sent offer");
      }
    };

    pc.onicecandidate = (ev) => {
      if (!ev.candidate || ws.readyState !== 1) return;
      ws.send(
        JSON.stringify({
          type: "ice",
          session_id: state.session.id,
          from: state.dogId,
          candidate: ev.candidate.candidate,
          sdp_mid: ev.candidate.sdpMid,
          sdp_mline_index: ev.candidate.sdpMLineIndex,
        })
      );
    };

    ws.onmessage = async (ev) => {
      let msg;
      try { msg = JSON.parse(ev.data); } catch { return; }
      if (msg.from === state.dogId) return;
      if (msg.type === "offer" && !isOfferer) {
        await pc.setRemoteDescription({ type: "offer", sdp: msg.sdp });
        const answer = await pc.createAnswer();
        await pc.setLocalDescription(answer);
        ws.send(
          JSON.stringify({
            type: "answer",
            session_id: state.session.id,
            from: state.dogId,
            sdp: answer.sdp,
          })
        );
        log("sent answer");
      } else if (msg.type === "answer" && isOfferer) {
        await pc.setRemoteDescription({ type: "answer", sdp: msg.sdp });
        log("got answer");
      } else if (msg.type === "ice") {
        try {
          await pc.addIceCandidate({
            candidate: msg.candidate,
            sdpMid: msg.sdp_mid,
            sdpMLineIndex: msg.sdp_mline_index,
          });
        } catch (e) {
          log("ice err " + e.message);
        }
      }
    };
  }

  async function done() {
    if (state.session) {
      await fetch(`${state.api}/v1/sessions/${state.session.id}/end`, {
        method: "POST",
        headers: authHeaders(),
        body: JSON.stringify({
          dog_id: state.dogId,
          terminal_id: state.terminalId,
          reason: "done",
        }),
      });
    }
    if (state.pc) state.pc.close();
    if (state.ws) state.ws.close();
    if (state.localStream) state.localStream.getTracks().forEach((t) => t.stop());
    setUx("idle");
    $("btnAccept").disabled = true;
    log("done");
  }

  $("btnSave").onclick = () => saveForm();
  $("btnPresent").onclick = () => publishPresent().catch((e) => log(e.message));
  $("btnCall").onclick = () => callFriend().catch((e) => log(e.message));
  $("btnAccept").onclick = () => accept().catch((e) => log(e.message));
  $("btnDone").onclick = () => done().catch((e) => log(e.message));
  $("btnPoll").onclick = () => pollIncoming().catch((e) => log(e.message));

  // auto-poll lure when present
  setInterval(() => {
    if (state.token && state.ux !== "ringing_out" && state.ux !== "in_session" && state.ux !== "in_session_video") {
      pollIncoming().catch(() => {});
    }
  }, 1500);

  // deep link / query params from Android intent
  const params = new URLSearchParams(location.search);
  ["api", "signal", "token", "terminalId", "dogId"].forEach((k) => {
    if (params.get(k)) {
      const map = { api: "apiBase", signal: "signalBase", token: "token", terminalId: "terminalId", dogId: "dogId" };
      const sk = { api: "api", signal: "signal", token: "token", terminalId: "terminalId", dogId: "dogId" }[k];
      state[sk] = params.get(k);
      localStorage.setItem(map[k] === "apiBase" ? "apiBase" : map[k] === "signalBase" ? "signalBase" : k, params.get(k));
    }
  });
  if (params.get("api")) state.api = params.get("api");
  if (params.get("signal")) state.signal = params.get("signal");
  loadForm();
  log("portal ready (phone = dog camera+screen)");
  (async function autostart() {
    const params = new URLSearchParams(location.search);
    if (params.get("autostart") !== "1") return;
    try {
      await publishPresent();
      if (params.get("session") && params.get("role")) {
        state.session = { id: params.get("session") };
        $("sessionId").textContent = state.session.id;
        const isOfferer = params.get("role") === "offerer";
        state.role = params.get("role");
        await mediaReady(true);
        await startWebRtc(isOfferer);
        log("autostart WebRTC as " + state.role);
      }
    } catch (e) { log("autostart error: " + e.message); }
  })();
})();
