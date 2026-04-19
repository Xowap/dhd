# DHD Protocol Specification

The Dial Hifi Device (DHD) uses a JSON-based protocol over a CDC-ACM (Virtual Serial) link to communicate between the firmware and the host daemon.

## Transport Layer

- **Physical Layer**: USB 2.0 (Full Speed)
- **Class**: CDC-ACM
- **Baud Rate**: 115,200 (Software-emulated)
- **Framing**: Newline-delimited (`\n`) JSON objects.

## Data Frames

### 1. Volume Update (`volume`)

Sent from **Device → Host** when the physical potentiometer position changes significantly.

- **Payload**:
  ```json
  { "volume": { "value": 0.752 } }
  ```
- **Range**: `0.0` to `1.0` (f32)
- **System Impact**: The host daemon maps this value linearly to the default PulseAudio sink's volume (0% to 100%).

### 2. Log Message (`log`)

Sent from **Device → Host** for diagnostic purposes.

- **Payload**:
  ```json
  { "log": { "level": "INFO", "message": "ADC calibration complete" } }
  ```
- **Levels**: `INFO`, `WARN`, `ERROR`
- **System Impact**: Displayed in the host terminal with appropriate color coding.

### 3. Handshake (`handshake`)

Sent from **Host → Device** immediately after connection to verify the device type and firmware version.

- **Payload**:
  ```json
  { "handshake": { "message": "Tek'ma'te Teal'c" } }
  ```
- **System Impact**: The device must respond with the correct reply message to confirm its identity. If the host does not receive the expected reply within 2 seconds, it will disconnect and retry.

### 4. Handshake Response (`handshake`)

Sent from **Device → Host** in response to a host handshake challenge.

- **Payload**:
  ```json
  { "handshake": { "message": "Tek'ma'te Bra'tac" } }
  ```
- **System Impact**: Completes the initial connection phase and allows the host to start processing volume and log messages.

### 5. Ping (`ping`)

Sent from **Host → Device** every 1 second to verify the connection health.

- **Payload**:
  ```json
  { "ping": { "timestamp": 1711468800000 } }
  ```
- **System Impact**: The device must respond immediately with a `pong`. If the host misses 3 consecutive pings (3 seconds total), it considers the connection dead and initiates a reconnection.

### 6. Pong (`pong`)

Sent from **Device → Host** in response to a `ping`.

- **Payload**:
  ```json
  { "pong": { "timestamp": 1711468800000 } }
  ```
- **System Impact**: The host verifies that the timestamp matches the last sent `ping` to confirm the round-trip link is healthy. Successful pong resets the "missed pings" counter to 0.

## Communication Sequences

### Volume Adjustment

When the user turns the dial, the device sends updates. The host maps these directly to the system's audio interface.

```mermaid
sequenceDiagram
    participant User
    participant Device as DHD Firmware
    participant Host as dhd Host Tool
    participant PA as PulseAudio / PipeWire

    User->>Device: Turn Potentiometer
    Device->>Device: Sample ADC & Filter
    Device->>Host: {"volume": {"value": 0.42}}
    Host->>PA: pa_context_set_sink_volume_by_index(...)
    PA-->>User: (Volume Audibly Changes)
```

### Health Monitoring (Heartbeat)

The host ensures the device is still responsive.

```mermaid
sequenceDiagram
    participant Host as dhd Host Tool
    participant Device as DHD Firmware

    loop Every 1 Second
        Host->>Device: {"ping": {"timestamp": 12345}}
        Device-->>Host: {"pong": {"timestamp": 12345}}
        Note right of Host: Missed 3? Reconnect.
    end
```

### Initialization & Handshake

On boot and connection, the host and device perform a handshake to ensure compatibility.

```mermaid
sequenceDiagram
    participant Host as dhd Host Tool
    participant Device as DHD Firmware

    Host->>Device: {"handshake": {"message": "Tek'ma'te Teal'c"}}
    Device-->>Host: {"handshake": {"message": "Tek'ma'te Bra'tac"}}
    Note over Host,Device: Connection Established

    Device->>Host: {"log": {"level": "INFO", "message": "Booting..."}}
    Host->>Host: Display Log
```
