
#[derive(Clone)]
pub enum Method {
    GET,
}

#[derive(Clone)]
struct StringRange {
    offset : usize,
    len : usize
}

#[derive(Clone)]
pub struct RequestLine {
    pub method : Method,
    request_uri : StringRange,
    http_version : StringRange,
}

pub struct Header {
    name : StringRange,
    value : StringRange,
}

pub struct Request {
    pub request_line : RequestLine,
    pub raw_request : String,
    pub headers: Vec<Header>
}

pub struct Response { }

pub struct ResponseWithStatusLine {
    bytes: Vec<u8>,
}

pub struct ResponseComplete {
    bytes: Vec<u8>,
}

pub struct RequestHeaderIterator<'a> {
    raw_request: &'a str,
    headers: &'a [Header],
}


impl <'a> Iterator for RequestHeaderIterator<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.headers.len() == 0 {
            None
        }else{
            let first = &self.headers[0];
            self.headers = &self.headers[1..];
            let nr = first.name.clone();
            let vr = first.value.clone();
            let name = &self.raw_request[nr.offset .. nr.offset + nr.len];
            let value = &self.raw_request[vr.offset .. vr.offset + vr.len];
            Some((name.trim(), value.trim()))
        }
    }
}


impl Response {
    pub fn status(version: &str, status : &str, code: u16) -> ResponseWithStatusLine {
        // Status-Line = HTTP-Version SP Status-Code SP Reason-Phrase CRLF
        let status_str = format!("{version} {code} {status}\r\n");
        let mut bytes = Vec::new();
        for c in status_str.chars() {
            bytes.push(c as u8);
        }

        ResponseWithStatusLine::from(std::mem::take(&mut bytes))
    }
}

impl ResponseWithStatusLine {
    fn from(bytes: Vec<u8>) -> Self {
        Self {
            bytes: bytes,
        }
    }

    fn add_header<Type : std::fmt::Display>(self : &mut Self, name : &str, value: Type) {
        let value = format!("{name}: {value}\r\n");
        for c in value.chars() {
            self.bytes.push(c as u8);
        }

    }

    pub fn header<Type : std::fmt::Display>(self : &mut Self, name : &str, value: Type) -> Self {
        assert!(name != "Content-Length");
        self.add_header(name, value);
        return Self::from(std::mem::take(&mut self.bytes));
    }

    pub fn payload(self : &mut Self, bytes: &[u8]) -> ResponseComplete {
        self.add_header("Content-Length", bytes.len());
        for c in "\r\n".chars() {
            self.bytes.push(c as u8);
        }

        for c in bytes {
            self.bytes.push(*c);
        }
        ResponseComplete::from(std::mem::take(&mut self.bytes))
    }

    pub fn build(self : &mut Self) -> ResponseComplete {
        for c in "\r\n".chars() {
            self.bytes.push(c as u8);
        }

        ResponseComplete::from(std::mem::take(&mut self.bytes))
    }
}

impl ResponseComplete {
    fn from(bytes: Vec<u8>) -> Self {
        Self {
            bytes: bytes
        }
    }

    pub fn send<Sender : std::io::Write>(&self, out: &mut Sender) -> bool {
        let mut buf : &[u8] = &self.bytes;

        while buf.len() > 0 {
            if let Ok(written) = out.write(buf) {
                buf = &buf[written..];
            }else{
                return false;
            }
        }
        return true;
    }
}

impl Request {
    fn from(raw_text : String) -> Result<Self, ParseError> {
        let req_line_len = Self::line_len(&raw_text);
        let request_line = parse_request_line(&raw_text[0..req_line_len])?;

        let headers_lines = if req_line_len + 2 <= raw_text.len() {
            &raw_text[req_line_len + 2..]
        }else{
            &raw_text[raw_text.len()..]
        };

        let headers = parse_headers(headers_lines, raw_text.len() - headers_lines.len())?;

        Ok(Self {
            request_line : request_line,
            raw_request : raw_text,
            headers : headers,
        })
    }

    pub fn headers<'a>(self: &'a Self) -> RequestHeaderIterator<'a> {
        return RequestHeaderIterator{raw_request: &self.raw_request, headers: &self.headers};
    }


    fn line_len(s : &str) -> usize {
        if s.len() <= 2 {
            return s.len();
        }
        for i in 0..s.len() - 1 {
            let newline = &s[i..i+2];
            if newline == "\r\n" {
                return i;
            }
        }
        return s.len();
    }

    fn to_slice(self : &Self, range : StringRange) -> &str {
        return &self.raw_request[range.offset..range.offset + range.len];
    }
    
    pub fn get_uri(self : &Self) -> &str {
        return self.to_slice(self.request_line.request_uri.clone());
    }

    pub fn get_http_version(self : &Self) -> &str {
        return self.to_slice(self.request_line.http_version.clone());
    }

    pub fn get_header(self : &Self, name : &str) -> Option<&str> {
        for header in &self.headers {
            let slice = self.to_slice(header.name.clone());
            if slice == name {
                return Some(self.to_slice(header.value.clone()).trim());
            }
        }
        None
    }
}


#[derive(Debug)]
pub enum ParseError {
    Io(std::io::Error),
    Utf(std::str::Utf8Error),
    InvalidRequest(String)
}

impl StringRange {
    fn from_indices(start : usize, end : usize) -> Self {
        assert!(start <= end);
        Self {
            offset : start,
            len : end - start
        }
    }

    #[allow(dead_code)]
    fn from(offset : usize, len : usize) -> Self {

        Self {
            offset : offset,
            len : len
        }
    }
}

impl From<std::io::Error> for ParseError {
    fn from(value: std::io::Error) -> Self {
        return Self::Io(value);
    }
}

impl From<std::str::Utf8Error> for ParseError {
    fn from(value: std::str::Utf8Error) -> Self {
        return Self::Utf(value);
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Utf(utf) => utf.fmt(f),
            Self::Io(io) => io.fmt(f),
            Self::InvalidRequest(msg) => msg.fmt(f),
        }
    }
}

#[allow(dead_code)]
fn is_ctl(c : char) -> bool { c as usize <= 31 || c as usize == 127 }
#[allow(dead_code)]
fn is_upalpha(c : char) -> bool {c >= 'A' && c <= 'Z'}
///
///```
///assert_eq!(is_loalpha('1'), false);
///```
#[allow(dead_code)]
fn is_loalpha(c : char) -> bool {c >= 'a' && c <= 'z'}

#[allow(dead_code)]
fn is_ascii_char(c : char) -> bool {c as i32 <= 127 && c as i32 >= 0}

fn next_line(s : &str) -> Option<(&str, &str)> {
    if let Some(idx) = s.find("\r\n") {
        return Some((&s[0..idx], &s[idx + 2..]));
    }else{
        return None; 
    }
}

fn parse_headers(mut headers_lines : &str, offset: usize) -> Result<Vec<Header>, ParseError> {
    let mut skipped = offset;
    let mut headers = Vec::new();
    while let Some((line, rest)) = next_line(headers_lines) {
        headers_lines = rest;

        if line.len() == 0 {
            return Ok(headers);
        }
       
        if let Some(delim) = line.find(':') {
            headers.push(Header{
                name: StringRange::from_indices(skipped, skipped + delim),
                value: StringRange::from_indices(skipped + delim + 1, skipped + line.len()),
            });
        }else{
            return Err(ParseError::InvalidRequest(String::from(format!("Header '{}' does not contain colon!", line))));
        }
        skipped += line.len() + 2;
    }
    return Err(ParseError::InvalidRequest(String::from("Didn't find two consecutives CRLFs")));
}

fn parse_request_line<'a>(text : &'a str) -> Result<RequestLine, ParseError> {
    let line = if let Some(line_end) = text.find("\r\n") {
        &text[..line_end]
    }else{
        text
    };
    //  Request-Line   = Method SP Request-URI SP HTTP-Version CRLF
    let idx0 = if let Some(first_index) = line.find(' ') {
        first_index
    }else{
        return Err(ParseError::InvalidRequest(String::from("Didn't find two spaces in Request-Line")));
    };

    let idx1 = if let Some(second_index) = (&line[idx0 + 1..]).find(' ') {
        second_index + idx0 + 1
    }else{
        return Err(ParseError::InvalidRequest(String::from("Didn't find two spaces in Request-Line")));
    };

    assert!(line.chars().nth(idx0) == Some(' '));
    assert!(line.chars().nth(idx1) == Some(' '));

    let method_str = &line[0..idx0];

    let m = match method_str {
        "GET" => Method::GET,
        _ => {
            return Err(ParseError::InvalidRequest(String::from("Invalid Method")));
        }
    };


    return Ok(RequestLine {
        method : m,
        request_uri : StringRange::from_indices(idx0 + 1, idx1),
        http_version : StringRange::from_indices(idx1 + 1, line.len()),
    });
}

pub fn parse_request<Reader : std::io::Read>(reader : &mut Reader) -> Result<Request, ParseError> {
    let mut request_text = String::new();
    let mut buffer = [0; 1024];
    let mut payload_offset : Option<usize> = None;
    while payload_offset == None {
        let count = reader.read(&mut buffer)?;
        buffer[count] = 0;
        if count == 0 {
            break; // no more bytes available. For TcpStream: the connection has been shutdown.
        }

        let append = std::str::from_utf8(&buffer)?;
        request_text += &append[0..count];
        let search_slice = if request_text.len() >= append.len() + 3 {
            &request_text[request_text.len() - append.len() - 3..]
        }else{
            &request_text
        };
        

        for k in 0..search_slice.len() - 3 {
            let candidate = &search_slice[k..k + 4];
            if candidate == "\r\n\r\n" {
                payload_offset = Some(0); // TODO: calculate payload start
                break;
            }
        }
    }

    
    return Ok(Request::from(request_text)?);
}


