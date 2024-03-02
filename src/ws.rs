use crate::http;

use sha1::{Sha1, Digest};

pub struct Websocket <Connection : std::io::Read + std::io::Write> {
    closed : bool,
    connection : Connection,
    incomplete_fragment: IncompleteFragment,
    incomplete_message: IncompleteMessage,
}

struct IncompleteMessage {
    opcode : u8,
    bytes: Vec<u8>
}

struct Fragment {
    bytes: Vec<u8>,
    payload_offset : usize
}

struct IncompleteFragment {
    bytes: Vec<u8>
}

pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<u16>),
}



fn base64_convert(block : u8) -> char {
    assert!(block < 64);
    if block < 26 {
        return (b'A' + block) as char;
    }
    if block < 52 {
        return (b'a' + block - 26) as char;
    }
    if block < 62 {
        return (b'0' + block - 52) as char;
    }
    if block == 62 {
        return '+';
    }
    return '/';
}

fn hash_to_base64(bytes : &[u8]) -> String {
    assert!(bytes.len() == 20);
    let mut buffer = String::new();
    buffer.reserve_exact(28);
    for i in (0..bytes.len()).step_by(3) {
        if i + 1 >= bytes.len() {
            // two padding bytes
            let byte0 : u8 = bytes[i];
            let byte1 : u8 = 0;

            let b0 : u8 = (byte0 >> 2) & 0x3F;
            let b1 : u8 = ((byte0 << 4) | (byte1 >> 4)) & 0x3F;

            buffer.push(base64_convert(b0));
            buffer.push(base64_convert(b1));
            buffer.push('=');
            buffer.push('=');
        }else if i + 2 >= bytes.len() {

            let byte0 : u8 = bytes[i + 0];
            let byte1 : u8 = bytes[i + 1];
            let byte2 : u8 = 0;

            let b0 : u8 = (byte0 >> 2) & 0x3F;
            let b1 : u8 = ((byte0 << 4) | (byte1 >> 4)) & 0x3F;
            let b2 : u8 = ((byte1 << 2) | (byte2 >> 6)) & 0x3F;

            buffer.push(base64_convert(b0));
            buffer.push(base64_convert(b1));
            buffer.push(base64_convert(b2));
            buffer.push('=');
        }else{
            let byte0 : u8 = bytes[i + 0];
            let byte1 : u8 = bytes[i + 1];
            let byte2 : u8 = bytes[i + 2];

            let b0 : u8 = (byte0 >> 2) & 0x3F;
            let b1 : u8 = ((byte0 << 4) | (byte1 >> 4)) & 0x3F;
            let b2 : u8 = ((byte1 << 2) | (byte2 >> 6)) & 0x3F;
            let b3 : u8 = byte2 & 0x3F;

            buffer.push(base64_convert(b0));
            buffer.push(base64_convert(b1));
            buffer.push(base64_convert(b2));
            buffer.push(base64_convert(b3));
        }
    }
    return buffer;
}

pub fn upgrade<Connection : std::io::Read + std::io::Write>(mut conn : Connection, req : &http::Request) -> Option<Websocket<Connection>> {
    let key = if let Some(tmp) = req.get_header("Sec-WebSocket-Key") {tmp} else {assert!(false); return None; };

    let hash = {
        let concat = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        let mut hasher = Sha1::new();
        hasher.update((format!("{key}{concat}")).as_bytes());
        hasher.finalize()
    };

    let b64 = hash_to_base64(&hash as &[u8]);
    assert!(b64.len() == 28);

    http::Response::status(req.get_http_version(), "Switching Protocols", 101)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Accept", &b64)
        .build()
        .send(&mut conn);
    
    Some(Websocket::<Connection>::from(conn))
}


impl<Connection: std::io::Read + std::io::Write> From<Connection> for Websocket<Connection> {
    fn from(conn: Connection) -> Websocket<Connection> {
        Websocket::<Connection> {
            closed: false,
            connection : conn,
            incomplete_fragment: IncompleteFragment {bytes: Vec::new()},
            incomplete_message: IncompleteMessage{bytes: Vec::new(), opcode: 0},
        }
    }
}

impl IncompleteFragment {

    const MIN_SIZE : usize = 2;

    fn is_masked(&self) -> Option<bool> {
        if self.bytes.len() < 2 {
            None
        }else{
            Some((self.bytes[1] >> 7) != 0)
        }
    }

    fn provisional_payload_length(&self) -> Option<u8> {
        if self.bytes.len() < 2 {
            None
        }else{
            Some(self.bytes[1] & 0x7F)
        }
    }

    fn try_append_nbytes(&mut self, n : usize, bytes: &mut &[u8]) -> bool {
        for i in 0..n {
            if bytes.len() == 0 {
                *bytes = &(*bytes)[i..];
                return false;
            }

            self.bytes.push(bytes[i]);
        }

        *bytes = &(*bytes)[n..];
        return true;
    }

    fn get_length_till_end_of_payload(&self) -> Option<usize> {
        let provisional = self.provisional_payload_length();
        if provisional == None {return None; }
        let res = match self.provisional_payload_length().unwrap() {
            126 => {
                // 16 bit extended payload length
                Self::MIN_SIZE + 2 
            },
            127 => {
                // 64 bit extended payload length
                Self::MIN_SIZE + 8
            },
            _ => {Self::MIN_SIZE}
        };
        return Some(res);
    }

    fn get_mask(&self) -> Option<[u8;4]> {
        let payload_len_end = self.get_length_till_end_of_payload()?;
        if self.bytes.len() <= payload_len_end + 4 || !self.is_masked().unwrap() {
            return None;
        }

        let mask = &self.bytes[payload_len_end..payload_len_end + 4];
        let mut buf = [0u8; 4];
        buf.clone_from_slice(mask);
        return Some(buf);
    }

    fn payload_len(&self) -> Option<usize> {
        let payload_len_end = self.get_length_till_end_of_payload()?;
        if self.bytes.len() < payload_len_end {
            return None;
        }

        let res = match self.provisional_payload_length()? {
            126 => {
                assert!(payload_len_end == 4);
                let bytes = &self.bytes[2..4];
                let mut buf = [0u8; 2];
                buf[0..2].clone_from_slice(bytes);
                u16::from_be_bytes(buf) as usize
            },
            127 => {
                assert!(payload_len_end == 10);
                let bytes = &self.bytes[2..10];
                let mut buf = [0u8; 8];
                buf[0..8].clone_from_slice(bytes);
                u64::from_be_bytes(buf) as usize
            },
            v => v as usize
        };
        return Some(res);
    }


    fn append(&mut self, data : &mut &[u8]) -> Result<Option<Fragment>, Error> {

        if self.bytes.len() < Self::MIN_SIZE {
            if !self.try_append_nbytes(Self::MIN_SIZE - self.bytes.len(), data) {
                return Ok(None);
            }
        }
        // is_masked, opcode, provisional_payload_length is now available

        assert!(self.bytes.len() >= Self::MIN_SIZE);

        let till_end_of_extended_payload_len = self.get_length_till_end_of_payload().unwrap();
        if self.bytes.len() < till_end_of_extended_payload_len {
            let remaining = till_end_of_extended_payload_len - self.bytes.len();
            if !self.try_append_nbytes(remaining, data) {
                return Ok(None);
            }
        }

        assert!(self.bytes.len() >= till_end_of_extended_payload_len);

        // payload length is now available

        let end_of_mask = if self.is_masked().unwrap() {
            let end_of_mask = till_end_of_extended_payload_len + 4;
            if self.bytes.len() < end_of_mask {
                let remaining = end_of_mask - self.bytes.len();
                if !self.try_append_nbytes(remaining, data) {
                    return Ok(None);
                }
            }
            end_of_mask
        }else{
            till_end_of_extended_payload_len
        };

        // mask is now available if it exists

        // everything in the 'header' is available
        // Now reading payload data
        let payload_len = self.payload_len().unwrap();
        let end_of_fragment = end_of_mask + payload_len;

        assert!(self.bytes.len() < end_of_fragment);

        if !self.try_append_nbytes(end_of_fragment - self.bytes.len(), data) {
            return Ok(None);
        }

        assert!(end_of_mask + self.payload_len().unwrap() == self.bytes.len());

        if let Some(mask) = self.get_mask() {
            // mask bytes
            let payload_data = &mut self.bytes[end_of_mask..];
            for i in 0..payload_data.len() {
                let j = i % 4;
                payload_data[i] ^= mask[j];
            }
        }

        return Ok(Some(
                Fragment{
                    bytes: std::mem::take(&mut self.bytes),
                    payload_offset: end_of_mask
                }
        ));
    }
}

impl Fragment {
    fn payload(&self) -> &[u8] {
        &self.bytes[self.payload_offset..]
    }

    fn is_fin(&self) -> bool {
        (self.bytes[0] >> 7) != 0
    }

    fn opcode(&self) -> u8 {
        self.bytes[0] & 0xF
    }

    fn is_control_frame(&self) -> bool {
        (self.opcode() >> 3) != 0
    }
}

impl IncompleteMessage {

    fn accepts_opcode(&self, opcode: u8) -> bool {
        if self.bytes.len() == 0 {
            match opcode {
                0x1 => true, // text
                0x2 => true, // binary
                0x8 => true, // close
                0x9 => true, // ping
                0xA => true, // pong
                _ => false
            }
        }else{
            opcode == 0x0 // continuation
        }
    }


    fn append_fragment(&mut self, fragment: Fragment) -> Result<Option<Message>, Error> {
        if !self.accepts_opcode(fragment.opcode()) {
            return Err(Error::WebsocketError("unexpected opcode"));
        }

        if self.bytes.len() == 0 {
            self.opcode = fragment.opcode();
        }

        // append payload
        self.bytes.extend_from_slice(fragment.payload());

        if !fragment.is_fin() {
            return Ok(None);
        }

        return Ok(Some(Message::from(std::mem::take(&mut self.bytes), self.opcode)?));
    }
}

impl Message {
    fn from(data : Vec<u8>, opcode : u8) -> Result<Message, Error> {
        match opcode {
            0x1 => {
                if let Ok(s) = std::str::from_utf8(&data) {
                    Ok(Self::Text(s.to_string()))
                }else{
                    Err(Error::WebsocketError("expected payload to be ut8 encoded"))
                }
            },
            0x2 => {
                Ok(Self::Binary(data))
            },
            _ => {
                Err(Error::WebsocketError("Unsupported opcode"))
            }
        }
    }
}

#[derive(Debug)]
pub enum Error {
    IoError(std::io::Error),
    WebsocketError(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::IoError(e) => e.fmt(f),
            Self::WebsocketError(e) => e.fmt(f),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err : std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl<Connection: std::io::Read + std::io::Write> Websocket<Connection> {

    pub fn is_closed(&self) -> bool { self.closed }

    pub fn read(&mut self) -> Result<Vec<Message>, Error> {
        let mut buffer = [0; 1024];

        let mut messages = Vec::new();
        
        let nread = match self.connection.read(&mut buffer) {
            Ok(nread) => Ok(nread),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    Ok(0)
                }else{
                    Err(Error::IoError(e))
                }
            }
        }?;

        let mut received = &buffer[0..nread];

        while received.len() > 0 {
            if let Some(fragment) = self.incomplete_fragment.append(&mut received)? {
                if fragment.is_control_frame() {
                    // handle control frame
                    if fragment.opcode() == 0x8 {
                        // close frame
                        if fragment.payload().len() >= 2 {
                            let mut buf = [0u8; 2];
                            buf.clone_from_slice(&fragment.payload()[0..2]);
                            let code = u16::from_be_bytes(buf);
                            messages.push(Message::Close(Some(code)));
                        }else{
                            messages.push(Message::Close(None));
                        }
                        self.closed = true;
                        break;
                    }else if fragment.opcode() == 0x9 {
                        // ping frame
                        self.send(0xA, fragment.payload())?;
                    }
                }else if let Some(msg) = self.incomplete_message.append_fragment(fragment)? {
                    messages.push(msg);
                }
            }
        }
        return Ok(messages);
    }


    fn send(&mut self, opcode : u8, mut data : &[u8]) -> Result<(), Error> {
        let mut header = [0u8, 16];

        header[0] = (1 << 7) | (opcode & 0xF);


        let offset = if data.len() < 126 {
            // one byte payload length
            header[1] = (data.len() & 0x7F) as u8;
            2
        }else if data.len() <= 0xFFFF {
            // two byte extended payload length
            header[1] = 126;
            let bytes = (data.len() as u16).to_be_bytes();
            let payload_len = &mut header[2..4];
            payload_len.clone_from_slice(&bytes);
            4
        }else{
            // 8 byte extended payload length
            header[1] = 127;
            let bytes = (data.len() as u64).to_be_bytes();
            let payload_len = &mut header[2..10];
            payload_len.clone_from_slice(&bytes);
            10
        };

        let mut hdr = &header[0..offset];
        while hdr.len() > 0 {
            let nread = self.connection.write(hdr)?;
            hdr = &hdr[nread..];
        }

        // send payload
        while data.len() > 0 {
            let nread = self.connection.write(data)?;
            data = &data[nread..];
        }
        Ok(())
    }

    pub fn send_text(&mut self, data : &str) -> Result<(), Error> {
        self.send(0x1, data.as_bytes())
    }

    pub fn send_bytes(&mut self, data : &[u8]) -> Result<(), Error> {
        self.send(0x2, data)
    }

    pub fn close(&mut self, code : Option<u16>) -> Result<(), Error> {
        self.closed = true;
        if let Some(code) = code {
            self.send(0x8, &code.to_be_bytes())
        }else{
            self.send(0x8, &[0u8; 0])
        }
    }
}

