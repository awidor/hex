#!/usr/bin/env python3
"""Measure how long ElevenLabs Scribe takes on dictation-sized clips.

Hex pastes a dictation when its transcript arrives, so the time between
releasing the shortcut and the answer is what a person feels. This script
measures it for the two paths Hex has, with the same requests Hex sends:

  batch  POST /v1/speech-to-text, model scribe_v3, raw 16 kHz PCM upload.
         Timed from sending the request to reading the transcript.
  live   WebSocket /v1/speech-to-text/realtime, model scribe_v3_live.
         The clip is streamed at speaking pace, then committed. Timed from
         the commit to the committed transcript (Settings > Final text: Live).

Usage:
  ELEVENLABS_API_KEY=... scripts/scribe-latency.py clip1.wav clip2.wav
  scripts/scribe-latency.py --runs 10 --live --language en short.wav long.wav

Clips must be 16 kHz mono 16-bit WAV. Convert anything else with:
  ffmpeg -i in.m4a -ar 16000 -ac 1 -c:a pcm_s16le out.wav

Every run is billed by ElevenLabs as audio transcribed (about $0.20 per hour
for batch and $0.22 per hour for live in November 2026). Ten runs of a 10 s
clip on both paths cost well under one cent. Only the standard library is
used. The key is read from ELEVENLABS_API_KEY, else from Hex's key file.
"""

import argparse
import base64
import json
import os
import select
import socket
import ssl
import statistics
import struct
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import wave

DEFAULT_BASE = "https://api.elevenlabs.io"
CHUNK_SAMPLES = 1_600  # 100 ms at 16 kHz
MID_COMMIT_SECONDS = 20.0


def api_key():
    key = os.environ.get("ELEVENLABS_API_KEY", "").strip()
    if key:
        return key
    data = os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share")
    try:
        with open(os.path.join(data, "hex", "elevenlabs.key"), encoding="utf-8") as file:
            return file.read().strip()
    except OSError:
        return ""


def read_pcm(path):
    with wave.open(path, "rb") as clip:
        shape = (clip.getframerate(), clip.getnchannels(), clip.getsampwidth())
        if shape != (16_000, 1, 2):
            sys.exit(
                f"{path}: need 16 kHz mono 16-bit WAV, got {shape[0]} Hz, "
                f"{shape[1]} channel(s), {shape[2] * 8}-bit. Convert with:\n"
                f"  ffmpeg -i {path} -ar 16000 -ac 1 -c:a pcm_s16le out.wav"
            )
        return clip.readframes(clip.getnframes())


def batch(base, key, pcm, language):
    boundary = f"hex-latency-{uuid.uuid4().hex}"
    fields = [
        ("model_id", "scribe_v3"),
        ("file_format", "pcm_s16le_16"),
        ("tag_audio_events", "false"),
        ("diarize", "false"),
        ("timestamps_granularity", "none"),
    ]
    if language:
        fields.append(("language_code", language))
    body = b"".join(
        f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode()
        for name, value in fields
    )
    body += (
        f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="audio.pcm"\r\n'
        "Content-Type: application/octet-stream\r\n\r\n"
    ).encode()
    body += pcm + f"\r\n--{boundary}--\r\n".encode()
    request = urllib.request.Request(
        f"{base}/v1/speech-to-text",
        data=body,
        method="POST",
        headers={
            "xi-api-key": key,
            "Content-Type": f"multipart/form-data; boundary={boundary}",
            "Accept": "application/json",
        },
    )
    started = time.monotonic()
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            answer = json.load(response)
    except urllib.error.HTTPError as error:
        sys.exit(f"batch request failed: HTTP {error.code}: {error.read(500).decode(errors='replace')}")
    except urllib.error.URLError as error:
        sys.exit(f"batch request failed: {error.reason}")
    return time.monotonic() - started, answer.get("text", "").strip()


class WebSocket:
    """The little of RFC 6455 a text-only client needs."""

    def __init__(self, url, headers):
        parts = urllib.parse.urlsplit(url)
        secure = parts.scheme == "wss"
        port = parts.port or (443 if secure else 80)
        raw = socket.create_connection((parts.hostname, port), timeout=15)
        raw.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.sock = (
            ssl.create_default_context().wrap_socket(raw, server_hostname=parts.hostname)
            if secure
            else raw
        )
        target = parts.path + (f"?{parts.query}" if parts.query else "")
        handshake = [
            f"GET {target} HTTP/1.1",
            f"Host: {parts.netloc}",
            "Upgrade: websocket",
            "Connection: Upgrade",
            f"Sec-WebSocket-Key: {base64.b64encode(os.urandom(16)).decode()}",
            "Sec-WebSocket-Version: 13",
        ] + [f"{name}: {value}" for name, value in headers.items()]
        self.sock.sendall(("\r\n".join(handshake) + "\r\n\r\n").encode())
        self.buffer = b""
        head = self._until(b"\r\n\r\n").decode(errors="replace")
        if " 101 " not in head.split("\r\n", 1)[0]:
            sys.exit(f"live session refused: {head.splitlines()[0]}")

    def _fill(self):
        data = self.sock.recv(65_536)
        if not data:
            raise ConnectionError("the live session closed")
        self.buffer += data

    def _until(self, marker):
        while marker not in self.buffer:
            self._fill()
        head, self.buffer = self.buffer.split(marker, 1)
        return head

    def _take(self, count):
        while len(self.buffer) < count:
            self._fill()
        data, self.buffer = self.buffer[:count], self.buffer[count:]
        return data

    def ready(self, timeout):
        """Whether a message can be read within `timeout` seconds."""
        if self.buffer or getattr(self.sock, "pending", lambda: 0)():
            return True
        readable, _, _ = select.select([self.sock], [], [], timeout)
        return bool(readable)

    def send(self, text):
        payload = text.encode()
        mask = os.urandom(4)
        if len(payload) < 126:
            head = struct.pack("!BB", 0x81, 0x80 | len(payload))
        elif len(payload) < 65_536:
            head = struct.pack("!BBH", 0x81, 0x80 | 126, len(payload))
        else:
            head = struct.pack("!BBQ", 0x81, 0x80 | 127, len(payload))
        masked = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
        self.sock.sendall(head + mask + masked)

    def receive(self):
        """The next text message, or None when the server closed."""
        message = b""
        while True:
            first, second = self._take(2)
            opcode, length = first & 0x0F, second & 0x7F
            if length == 126:
                (length,) = struct.unpack("!H", self._take(2))
            elif length == 127:
                (length,) = struct.unpack("!Q", self._take(8))
            payload = self._take(length)
            if opcode == 0x8:
                return None
            if opcode == 0x9:  # ping: answer with a masked pong
                mask = os.urandom(4)
                pong = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
                self.sock.sendall(struct.pack("!BB", 0x8A, 0x80 | len(payload)) + mask + pong)
                continue
            if opcode in (0x0, 0x1):
                message += payload
                if first & 0x80:
                    return message.decode(errors="replace")

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass


def chunk_message(pcm, commit):
    return json.dumps(
        {
            "message_type": "input_audio_chunk",
            "audio_base_64": base64.b64encode(pcm).decode(),
            "commit": commit,
            "sample_rate": 16_000,
        }
    )


def live(base, key, pcm, language):
    scheme, rest = base.split("://", 1)
    query = {
        "model_id": "scribe_v3_live",
        "audio_format": "pcm_16000",
        "commit_strategy": "manual",
    }
    if language:
        query["language_code"] = language
    url = (
        f"{'wss' if scheme == 'https' else 'ws'}://{rest}/v1/speech-to-text/realtime?"
        + urllib.parse.urlencode(query)
    )
    started = time.monotonic()
    session = WebSocket(url, {"xi-api-key": key})
    connected = time.monotonic() - started
    text = []
    sent = answered = 0

    def read_one():
        """Reads one message and counts it when it answers a commit."""
        nonlocal answered
        raw = session.receive()
        if raw is None:
            sys.exit("the live session closed before it answered the commit")
        message = json.loads(raw)
        kind = message.get("message_type", "")
        if kind.startswith("committed_transcript"):
            text.append(message.get("text", "").strip())
            answered += 1
        elif kind in ("insufficient_audio_activity", "commit_throttled"):
            answered += 1
        elif kind not in ("partial_transcript", "session_started", "warning"):
            sys.exit(f"live session failed: {kind}: {message.get('error', '')}")

    step = CHUNK_SAMPLES * 2
    uncommitted = 0.0
    begun = time.monotonic()
    for index, offset in enumerate(range(0, len(pcm), step)):
        # Speaking pace: chunk N is due N * 100 ms after the first.
        wait = begun + index * 0.1 - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        session.send(chunk_message(pcm[offset : offset + step], False))
        uncommitted += 0.1
        # Hex commits a long segment itself (at a pause; here simply every
        # 20 s), because ElevenLabs otherwise commits on its own after about
        # 35 s and the commit after that one comes back empty.
        if uncommitted >= MID_COMMIT_SECONDS:
            session.send(chunk_message(b"", True))
            sent += 1
            uncommitted = 0.0
    while session.ready(0.05):
        read_one()
    committed_at = time.monotonic()
    session.send(chunk_message(b"", True))
    sent += 1
    while answered < sent:
        read_one()
    latency = time.monotonic() - committed_at
    session.close()
    return connected, latency, " ".join(part for part in text if part)


def summary(seconds):
    ordered = sorted(seconds)
    p90 = ordered[min(len(ordered) - 1, round(0.9 * (len(ordered) - 1)))]
    return (
        f"min {ordered[0] * 1000:6.0f} ms   median {statistics.median(ordered) * 1000:6.0f} ms   "
        f"p90 {p90 * 1000:6.0f} ms   max {ordered[-1] * 1000:6.0f} ms"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("clips", nargs="+", help="16 kHz mono 16-bit WAV files")
    parser.add_argument("--runs", type=int, default=10, help="requests per clip and path (10)")
    parser.add_argument("--live", action="store_true", help="also measure the realtime model")
    parser.add_argument("--language", default="", help="ISO code; detected when omitted")
    parser.add_argument(
        "--base",
        default=os.environ.get("HEX_ELEVENLABS_URL", DEFAULT_BASE).rstrip("/"),
        help="API base URL",
    )
    arguments = parser.parse_args()
    key = api_key()
    if not key:
        sys.exit("No API key: set ELEVENLABS_API_KEY or run `hex key set`.")
    billed = 0.0
    for path in arguments.clips:
        pcm = read_pcm(path)
        seconds = len(pcm) / 32_000
        print(f"\n{path}  ({seconds:.1f} s of audio)")
        times, text = [], ""
        for _ in range(arguments.runs):
            elapsed, text = batch(arguments.base, key, pcm, arguments.language)
            times.append(elapsed)
            billed += seconds
        print(f"  batch  {summary(times)}")
        print(f"         {text[:100]!r}")
        if arguments.live:
            connects, latencies = [], []
            for _ in range(arguments.runs):
                connected, latency, text = live(arguments.base, key, pcm, arguments.language)
                connects.append(connected)
                latencies.append(latency)
                billed += seconds
            print(f"  live   {summary(latencies)}   (after the commit)")
            print(f"  live connect {summary(connects)}")
            print(f"         {text[:100]!r}")
    print(f"\nAudio sent: {billed / 60:.1f} minutes in total.")


if __name__ == "__main__":
    main()
