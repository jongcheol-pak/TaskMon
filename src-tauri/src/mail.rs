//! 메일 알림 (POP3) 모듈
//!
//! - POP3 명령(USER/PASS/STAT/UIDL/TOP/QUIT) 직접 구현 + native-tls (995 implicit TLS / 110 평문)
//! - 비밀번호는 Windows DPAPI로 암호화하여 `%LocalAppData%\TaskMon\mail_secret.bin`에 저장
//! - MIME 헤더(RFC 2047 인코딩) 파싱은 `mailparse` 크레이트 사용
//! - Date 헤더는 Windows API로 시스템 로컬 타임존 변환 후 `HH:MM` 표시

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

/// POP3 read/write/connect 타임아웃 (초)
const POP3_TIMEOUT_SECS: u64 = 15;
/// 한 번 폴링에서 가져올 최대 신규 메일 수 (서버 부하/말풍선 큐 폭주 방지)
const MAX_FETCH_PER_POLL: usize = 50;
/// POP3 명령 한 줄 최대 길이 (RFC 1939 권고)
const MAX_LINE_LEN: usize = 8192;
/// baseline UIDL 보존 기간 (일). 이 기간 이상 지나면 자동 정리되고,
/// 메일 자체 Date 헤더가 이 기간보다 과거면 알림이 발화되지 않는다.
pub const STALE_DAYS: i64 = 90;
/// 90일을 초 단위로 환산
pub const STALE_SECS: i64 = STALE_DAYS * 24 * 60 * 60;

// ===== 직렬화 타입 =====

/// 프런트 ↔ 백엔드 IPC용 설정 (비밀번호 포함)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailConfig {
    pub enabled: bool,
    #[serde(default)]
    pub account_name: String,
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub user_id: String,
    /// 평문 비밀번호. drop 시 자동 zeroize. 빈 문자열 = "기존 저장값 유지"
    #[serde(default)]
    pub password: String,
    pub poll_minutes: u32,
}

/// `MailConfig`가 drop될 때 password 평문을 메모리에서 자동 wipe.
/// IPC 경계에서 String으로 받는 구조라 호출처마다 명시적 zeroize를 잊을 위험을
/// 제거하기 위해 타입 자체에 일관된 wipe를 보장한다.
/// (clone된 인스턴스도 자기만의 password를 가지므로 각자 drop 시 wipe된다.)
impl Drop for MailConfig {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.password.zeroize();
    }
}

/// 비밀번호 제외 메타 (UI 빠른 복원용)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MailConfigMeta {
    pub enabled: bool,
    pub account_name: String,
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub user_id: String,
    pub poll_minutes: u32,
}

/// 설정 화면 로드용 응답
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MailConfigLoad {
    pub config: MailConfigMeta,
    pub has_password: bool,
}

/// baseline에 등록된 UIDL 1건 + 등록 시점 timestamp
/// `seen_at`이 STALE_DAYS 이상 과거이면 매 폴링 시작 시 자동 정리된다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UidlEntry {
    pub uidl: String,
    /// baseline에 처음 등록된 시점 (unix timestamp, 초)
    pub seen_at: i64,
}

/// DPAPI로 보호되어 디스크에 저장되는 데이터 묶음
/// 자격 증명·UIDL baseline을 한 파일에 묶어 동기화 깨짐을 방지한다.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredMailData {
    pub config: MailConfigMeta,
    /// DPAPI 암호화된 비밀번호. 빈 벡터 = 비밀번호 미저장
    pub password_dpapi: Vec<u8>,
    /// **(deprecated)** 구버전 baseline. 호환 로드용으로만 유지하며 새 저장에서는 사용하지 않는다.
    #[serde(default)]
    pub last_seen_uidls: Vec<String>,
    /// baseline UIDL + 등록 시점. STALE_DAYS 이상 과거 항목은 자동 정리.
    #[serde(default)]
    pub last_seen: Vec<UidlEntry>,
}

/// 신규 메일 1건 (UI 표시용)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailEntry {
    pub id: String,
    /// "HH:MM" 24시간 형식 (시스템 로컬 타임존)
    pub sent_at: String,
    pub from: String,
    pub subject: String,
}

/// 분류된 오류. raw POP3 응답을 그대로 노출하지 않도록 카테고리만 사용한다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message")]
pub enum MailError {
    /// 인증 실패 — 폴링 자동 정지 신호
    Auth,
    /// 일시적 네트워크 오류 — 다음 주기에 자동 재시도
    Network(String),
    /// 프로토콜/서버 오류 — 다음 주기에 자동 재시도
    Protocol(String),
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MailError::Auth => write!(f, "Auth"),
            MailError::Network(m) => write!(f, "Network: {}", m),
            MailError::Protocol(m) => write!(f, "Protocol: {}", m),
        }
    }
}

impl std::error::Error for MailError {}

// ===== 저장 파일 IO =====

/// 자격 증명 파일 경로 (`%LocalAppData%\TaskMon\mail_secret.bin`)
fn mail_data_path() -> PathBuf {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| String::from(r"C:\Users\Default\AppData\Local"));
    PathBuf::from(local_app_data).join("TaskMon").join("mail_secret.bin")
}

/// 디스크에서 저장된 메일 데이터를 읽어온다. 없으면 None.
/// 구버전 `last_seen_uidls`만 채워진 파일은 자동으로 신규 형식(`last_seen`)으로 마이그레이션한다.
pub fn load_stored() -> Option<StoredMailData> {
    let path = mail_data_path();
    let bytes = std::fs::read(&path).ok()?;
    let mut data = serde_json::from_slice::<StoredMailData>(&bytes).ok()?;
    if data.last_seen.is_empty() && !data.last_seen_uidls.is_empty() {
        let now = now_unix();
        data.last_seen = data
            .last_seen_uidls
            .drain(..)
            .map(|u| UidlEntry { uidl: u, seen_at: now })
            .collect();
        // 마이그레이션 결과를 즉시 디스크에 반영 (다음 폴링 시 정상 동작)
        let _ = save_stored(&data);
    }
    Some(data)
}

/// 현재 unix timestamp (초)
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 디스크에 메일 데이터를 저장한다.
/// 디렉터리 생성은 첫 호출에서 한 번만 수행한다 (이후 호출은 syscall 생략).
pub fn save_stored(data: &StoredMailData) -> Result<(), String> {
    static DIR_READY: AtomicBool = AtomicBool::new(false);
    let path = mail_data_path();
    if !DIR_READY.load(Ordering::Relaxed) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("디렉터리 생성 실패: {}", e))?;
        }
        DIR_READY.store(true, Ordering::Relaxed);
    }
    let bytes = serde_json::to_vec(data).map_err(|e| format!("직렬화 실패: {}", e))?;
    std::fs::write(&path, &bytes).map_err(|e| format!("쓰기 실패: {}", e))?;
    Ok(())
}

// ===== DPAPI (Windows 자격 증명 보호) =====

/// CryptProtectData / CryptUnprotectData 공통 호출부.
/// 입력 버퍼 보관, BLOB 셋업, 결과 복사, LocalFree 처리를 한 곳에 모은다.
/// API 호출만 호출자가 closure로 주입한다 (시그니처가 미세하게 다름).
#[cfg(windows)]
fn dpapi_call<F>(input: &[u8], op_name: &str, call: F) -> Result<Vec<u8>, String>
where
    F: FnOnce(
        &windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
        &mut windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
    ) -> windows::core::Result<()>,
{
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB;

    // in_buf은 API 호출 중 살아 있어야 한다 (in_blob.pbData가 raw pointer로 참조).
    let mut in_buf = input.to_vec();
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: in_buf.len() as u32,
        pbData: in_buf.as_mut_ptr(),
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    call(&in_blob, &mut out_blob).map_err(|e| format!("{} 실패: {}", op_name, e))?;

    unsafe {
        if out_blob.pbData.is_null() {
            return Err("DPAPI: 빈 결과".into());
        }
        let result =
            std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out_blob.pbData as *mut _));
        Ok(result)
    }
}

/// 평문 바이트를 DPAPI로 암호화한다 (현재 사용자 키에 묶임).
#[cfg(windows)]
pub fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::CryptProtectData;
    use windows::core::PCWSTR;
    dpapi_call(plain, "CryptProtectData", |in_blob, out_blob| unsafe {
        CryptProtectData(in_blob, PCWSTR::null(), None, None, None, 0, out_blob)
    })
}

/// DPAPI로 암호화된 바이트를 복호화한다.
#[cfg(windows)]
pub fn dpapi_unprotect(encrypted: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::CryptUnprotectData;
    dpapi_call(encrypted, "CryptUnprotectData", |in_blob, out_blob| unsafe {
        CryptUnprotectData(in_blob, None, None, None, None, 0, out_blob)
    })
}

#[cfg(not(windows))]
pub fn dpapi_protect(_plain: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI는 Windows 전용".into())
}
#[cfg(not(windows))]
pub fn dpapi_unprotect(_encrypted: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI는 Windows 전용".into())
}

// ===== POP3 클라이언트 =====

/// 일반 TCP 또는 TLS 위 stream
enum Pop3Stream {
    Plain(TcpStream),
    Tls(native_tls::TlsStream<TcpStream>),
}

impl Read for Pop3Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Pop3Stream::Plain(s) => s.read(buf),
            Pop3Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Pop3Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Pop3Stream::Plain(s) => s.write(buf),
            Pop3Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Pop3Stream::Plain(s) => s.flush(),
            Pop3Stream::Tls(s) => s.flush(),
        }
    }
}

struct Pop3Client {
    reader: BufReader<Pop3Stream>,
}

impl Pop3Client {
    /// POP3 서버에 연결하고 환영 메시지(+OK)까지 읽는다.
    fn connect(host: &str, port: u16, use_tls: bool) -> Result<Self, MailError> {
        let addr = format!("{}:{}", host, port);
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| MailError::Network(format!("주소 해석 실패: {}", e)))?
            .next()
            .ok_or_else(|| MailError::Network("주소 미해석".into()))?;

        let tcp = TcpStream::connect_timeout(
            &socket_addr,
            Duration::from_secs(POP3_TIMEOUT_SECS),
        )
        .map_err(|e| MailError::Network(format!("연결 실패: {}", e)))?;
        tcp.set_read_timeout(Some(Duration::from_secs(POP3_TIMEOUT_SECS)))
            .map_err(|e| MailError::Network(format!("타임아웃 설정 실패: {}", e)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(POP3_TIMEOUT_SECS)))
            .map_err(|e| MailError::Network(format!("타임아웃 설정 실패: {}", e)))?;

        let stream = if use_tls {
            let connector = native_tls::TlsConnector::new()
                .map_err(|e| MailError::Network(format!("TLS 초기화 실패: {}", e)))?;
            let tls_stream = connector
                .connect(host, tcp)
                .map_err(|e| MailError::Network(format!("TLS 핸드셰이크 실패: {}", e)))?;
            Pop3Stream::Tls(tls_stream)
        } else {
            Pop3Stream::Plain(tcp)
        };

        let mut client = Pop3Client {
            reader: BufReader::new(stream),
        };
        // 환영 메시지(+OK ...) 한 줄 읽기
        client.read_status_line()?;
        Ok(client)
    }

    fn send_cmd(&mut self, cmd: &str) -> Result<(), MailError> {
        if cmd.len() > MAX_LINE_LEN {
            return Err(MailError::Protocol("명령 길이 초과".into()));
        }
        let line = format!("{}\r\n", cmd);
        let stream = self.reader.get_mut();
        stream
            .write_all(line.as_bytes())
            .map_err(|e| MailError::Network(format!("쓰기 실패: {}", e)))?;
        stream
            .flush()
            .map_err(|e| MailError::Network(format!("flush 실패: {}", e)))?;
        Ok(())
    }

    /// 한 줄 읽기 (\r\n 포함된 그대로)
    fn read_line_raw(&mut self) -> Result<String, MailError> {
        let mut buf = String::new();
        self.reader
            .read_line(&mut buf)
            .map_err(|e| MailError::Network(format!("읽기 실패: {}", e)))?;
        if buf.is_empty() {
            return Err(MailError::Network("연결이 끊어짐".into()));
        }
        Ok(buf)
    }

    /// 상태 라인(+OK / -ERR) 읽기
    fn read_status_line(&mut self) -> Result<String, MailError> {
        let line = self.read_line_raw()?;
        let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
        if let Some(rest) = trimmed.strip_prefix("+OK") {
            Ok(rest.trim().to_string())
        } else if let Some(rest) = trimmed.strip_prefix("-ERR") {
            Err(MailError::Protocol(rest.trim().to_string()))
        } else {
            Err(MailError::Protocol(format!(
                "알 수 없는 응답: {}",
                trimmed
            )))
        }
    }

    /// multi-line 응답 읽기 (`\r\n.\r\n`으로 종료, dot-stuffing 해제)
    fn read_multiline(&mut self) -> Result<Vec<String>, MailError> {
        let mut lines = Vec::new();
        loop {
            let line = self.read_line_raw()?;
            let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
            if trimmed == "." {
                return Ok(lines);
            }
            // dot-stuffing: 클라이언트는 ".."로 시작하면 첫 "." 제거
            let actual = if let Some(stripped) = trimmed.strip_prefix('.') {
                stripped.to_string()
            } else {
                trimmed.to_string()
            };
            lines.push(actual);
        }
    }

    fn login(&mut self, user: &str, pass: &Zeroizing<String>) -> Result<(), MailError> {
        // USER/PASS 응답이 -ERR이면 인증 오류로 분류
        self.send_cmd(&format!("USER {}", user))?;
        self.read_status_line().map_err(|e| match e {
            MailError::Protocol(_) => MailError::Auth,
            other => other,
        })?;
        self.send_cmd(&format!("PASS {}", pass.as_str()))?;
        self.read_status_line().map_err(|e| match e {
            MailError::Protocol(_) => MailError::Auth,
            other => other,
        })?;
        Ok(())
    }

    /// UIDL 명령 — 메시지 번호와 고유 식별자 목록을 받아온다
    fn uidl_list(&mut self) -> Result<Vec<(u32, String)>, MailError> {
        self.send_cmd("UIDL")?;
        // 일부 서버는 UIDL 미지원 → -ERR
        self.read_status_line()?;
        let lines = self.read_multiline()?;
        let mut result = Vec::with_capacity(lines.len());
        for line in lines {
            let mut parts = line.splitn(2, ' ');
            let n: u32 = match parts.next().and_then(|p| p.parse().ok()) {
                Some(n) => n,
                None => continue,
            };
            let uidl = match parts.next() {
                Some(u) => u.trim().to_string(),
                None => continue,
            };
            if !uidl.is_empty() {
                result.push((n, uidl));
            }
        }
        Ok(result)
    }

    /// TOP <msgnum> 0 — 헤더만 받아온다 (본문 0줄)
    fn top_header(&mut self, msgnum: u32) -> Result<Vec<String>, MailError> {
        self.send_cmd(&format!("TOP {} 0", msgnum))?;
        self.read_status_line()?;
        self.read_multiline()
    }

    fn quit(&mut self) {
        let _ = self.send_cmd("QUIT");
        let _ = self.read_status_line();
    }
}

// ===== MIME 헤더 파싱 =====

/// 헤더 라인 배열에서 from / subject / sent_at(HH:MM) / sent_at_unix 추출
/// sent_at_unix가 0이면 Date 헤더가 없거나 파싱 실패 (90일 필터에서 통과 처리)
fn parse_header_lines(lines: &[String]) -> (String, String, String, i64) {
    // 헤더 텍스트 합치기 (CRLF 구분)
    let mut header_text = lines.join("\r\n");
    // mailparse는 빈 줄(헤더 종료) 이후를 본문으로 간주하므로 마지막에 빈 줄 추가
    header_text.push_str("\r\n\r\n");
    let header_bytes = header_text.as_bytes();

    let mut from = String::new();
    let mut subject = String::new();
    let mut sent_at = String::new();
    let mut sent_at_unix: i64 = 0;

    if let Ok((headers, _)) = mailparse::parse_headers(header_bytes) {
        for h in headers {
            let key = h.get_key_ref().to_ascii_lowercase();
            let value = h.get_value();
            match key.as_str() {
                "from" => from = extract_display_name(&value),
                // RFC 2047 multi-word 디코딩 시 워드 경계가 multi-byte 글자 중간일 경우
                // mailparse가 lossy 처리하여 \u{FFFD}로 대체하는 케이스가 있음 → 깨진 글자만 제거.
                "subject" => subject = value.trim().replace('\u{FFFD}', "").trim().to_string(),
                "date" => {
                    let trimmed = value.trim();
                    if let Ok(ts) = mailparse::dateparse(trimmed) {
                        sent_at_unix = ts;
                        sent_at = unix_ts_to_local_hhmm(ts).unwrap_or_default();
                    }
                }
                _ => {}
            }
        }
    }
    (from, subject, sent_at, sent_at_unix)
}

/// `"홍길동" <hong@example.com>` → "홍길동"
/// `<hong@example.com>` → "hong@example.com"
/// `hong@example.com` → "hong@example.com"
fn extract_display_name(field: &str) -> String {
    let trimmed = field.trim();
    if let Some(angle_start) = trimmed.find('<') {
        let display = trimmed[..angle_start].trim();
        // 양쪽 따옴표 제거
        let display = display.trim_matches('"').trim();
        if !display.is_empty() {
            return display.to_string();
        }
        if let Some(angle_end) = trimmed[angle_start..].find('>') {
            let email_start = angle_start + 1;
            let email_end = angle_start + angle_end;
            return trimmed[email_start..email_end].to_string();
        }
    }
    trimmed.to_string()
}

/// Unix timestamp → 시스템 로컬 타임존 `HH:MM` (Windows API 사용)
/// 시스템 타임존 bias를 적용 후 SYSTEMTIME으로 변환한다.
/// DST는 표준 시간 기준만 적용 (한국은 DST 미사용이므로 정확).
#[cfg(windows)]
fn unix_ts_to_local_hhmm(unix_ts: i64) -> Option<String> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{
        FileTimeToSystemTime, GetTimeZoneInformation, TIME_ZONE_INFORMATION,
    };

    if unix_ts < 0 {
        return None;
    }

    // 현재 시스템 타임존 bias 얻기 (분 단위, UTC = local + Bias)
    let mut tz_info = TIME_ZONE_INFORMATION::default();
    unsafe {
        let _ = GetTimeZoneInformation(&mut tz_info);
    }
    let bias_minutes = tz_info.Bias as i64;
    // UTC → 로컬 unix timestamp (offset 적용)
    let local_unix_ts = unix_ts - bias_minutes * 60;

    // unix epoch(1970-01-01) - windows epoch(1601-01-01) = 11_644_473_600 seconds
    let windows_ticks = local_unix_ts
        .checked_add(11_644_473_600)?
        .checked_mul(10_000_000)?;
    if windows_ticks < 0 {
        return None;
    }
    let local_ft = FILETIME {
        dwLowDateTime: (windows_ticks & 0xFFFF_FFFF) as u32,
        dwHighDateTime: ((windows_ticks >> 32) & 0xFFFF_FFFF) as u32,
    };

    let mut st = SYSTEMTIME::default();
    unsafe {
        FileTimeToSystemTime(&local_ft, &mut st).ok()?;
    }
    Some(format!("{:02}:{:02}", st.wHour, st.wMinute))
}

#[cfg(not(windows))]
fn unix_ts_to_local_hhmm(_unix_ts: i64) -> Option<String> {
    None
}

// ===== 폴링 진입점 =====

/// 폴링 결과
pub struct PollOutcome {
    /// 알림으로 표시할 신규 메일 (90일 이내)
    pub new_mails: Vec<MailEntry>,
    /// 다음 baseline에 들어갈 UIDL → seen_at 매핑.
    /// 기존 baseline에 있던 항목은 기존 seen_at 유지, 신규 항목은 now.
    pub next_baseline: HashMap<String, i64>,
}

/// 신규 메일을 폴링한다.
///
/// `prev_baseline`: 디스크/메모리에 있던 직전 baseline (이미 STALE_DAYS 항목 정리된 상태로 전달).
/// `is_first_poll`: true이면 알림 발화 없이 baseline만 등록 (첫 활성화 시 폭주 방지).
pub fn check_new_mails(
    cfg: &MailConfig,
    prev_baseline: &HashMap<String, i64>,
    is_first_poll: bool,
) -> Result<PollOutcome, MailError> {
    let pass = Zeroizing::new(cfg.password.clone());
    let mut client = Pop3Client::connect(&cfg.host, cfg.port, cfg.use_tls)?;
    client.login(&cfg.user_id, &pass)?;

    let uidl_list = match client.uidl_list() {
        Ok(list) => list,
        Err(e) => {
            client.quit();
            return Err(e);
        }
    };

    let now = now_unix();
    let stale_cutoff = now - STALE_SECS;

    let mut new_mails = Vec::new();
    let mut next_baseline: HashMap<String, i64> = HashMap::with_capacity(uidl_list.len());

    for (msgnum, uidl) in &uidl_list {
        if let Some(&seen_at) = prev_baseline.get(uidl) {
            // 기존 baseline 항목 — seen_at 유지
            next_baseline.insert(uidl.clone(), seen_at);
            continue;
        }

        // 첫 폴링: 알림 발화 없이 baseline만 등록
        if is_first_poll {
            next_baseline.insert(uidl.clone(), now);
            continue;
        }

        // 신규 후보 — 헤더 받기
        if new_mails.len() >= MAX_FETCH_PER_POLL {
            // 이번 폴링 상한 도달 — baseline 등록도 보류 (다음 폴링에서 처리)
            continue;
        }
        match client.top_header(*msgnum) {
            Ok(lines) => {
                let (from, subject, sent_at, sent_at_unix) = parse_header_lines(&lines);

                // 90일 이상 과거 메일은 알림 발화 X. baseline에는 등록 (다음 폴링에서 TOP 재호출 회피)
                if sent_at_unix > 0 && sent_at_unix < stale_cutoff {
                    next_baseline.insert(uidl.clone(), now);
                    continue;
                }

                new_mails.push(MailEntry {
                    id: uidl.clone(),
                    sent_at,
                    from,
                    subject,
                });
                next_baseline.insert(uidl.clone(), now);
            }
            Err(e) => {
                client.quit();
                return Err(e);
            }
        }
    }

    client.quit();
    Ok(PollOutcome { new_mails, next_baseline })
}

/// baseline에서 STALE_DAYS 이상 과거 항목을 제거한다.
/// 매 폴링 시작 시 호출.
pub fn prune_stale(baseline: &mut HashMap<String, i64>) {
    let cutoff = now_unix() - STALE_SECS;
    baseline.retain(|_, seen_at| *seen_at >= cutoff);
}

/// 입력 검증: poll_minutes / port / host / user_id 기본 무결성
pub fn validate_config(cfg: &MailConfig) -> Result<(), String> {
    if cfg.host.trim().is_empty() {
        return Err("호스트가 비어 있습니다".into());
    }
    if cfg.user_id.trim().is_empty() {
        return Err("아이디가 비어 있습니다".into());
    }
    if cfg.poll_minutes < 1 || cfg.poll_minutes > 60 {
        return Err("폴링 시간은 1~60분 사이여야 합니다".into());
    }
    if cfg.port == 0 {
        return Err("포트가 0입니다".into());
    }
    Ok(())
}

/// MailConfig → MailConfigMeta (저장용 변환, 비밀번호 제외)
pub fn meta_from_config(cfg: &MailConfig) -> MailConfigMeta {
    MailConfigMeta {
        enabled: cfg.enabled,
        account_name: cfg.account_name.clone(),
        host: cfg.host.clone(),
        port: cfg.port,
        use_tls: cfg.use_tls,
        user_id: cfg.user_id.clone(),
        poll_minutes: cfg.poll_minutes,
    }
}
