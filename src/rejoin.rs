//! Restart a Roblox client into the server it was in. A public server is read from Roblox's own log (the last
//! "Joining game" line, so teleports are followed); a private server from the link code in the command line
//! the client was started with. The client is then reopened with Roblox's own join link.

/// Where a client was.
#[derive(Clone, Debug, PartialEq)]
pub enum Server {
    /// a public server: the place and that exact server
    Public { place: u64, job: String },
    /// a private server, from a share link (`linkCode`) or a Roblox launch (`accessCode`)
    Private { place: u64, code: String, access: bool },
}

impl Server {
    /// Roblox's join link for it.
    pub fn link(&self) -> String {
        match self {
            Server::Public { place, job } => format!("roblox://experiences/start?placeId={place}&gameInstanceId={job}"),
            Server::Private { place, code, access: false } => format!("roblox://experiences/start?placeId={place}&linkCode={code}"),
            Server::Private { place, code, access: true } => format!("roblox://experiences/start?placeId={place}&accessCode={code}"),
        }
    }
}

/// The last server a client log says it joined: (place, server id).
pub fn last_join(log: &str) -> Option<(u64, String)> {
    log.lines().rev().find_map(|l| {
        let rest = &l[l.find("Joining game '")? + 14..];
        let job = &rest[..rest.find('\'')?];
        let place = rest[rest.find(" place ")? + 7..].split_whitespace().next()?.parse().ok()?;
        valid_id(job).then(|| (place, job.to_string()))
    })
}

/// The private server a client was launched into, from its command line.
pub fn private_server(command_line: &str) -> Option<Server> {
    let text = decode(&decode(command_line));   // the launch link is often URL-encoded inside another URL
    let place = param(&text, "placeId")?.parse().ok()?;
    if let Some(code) = param(&text, "linkCode") {
        return Some(Server::Private { place, code, access: false });
    }
    param(&text, "accessCode").map(|code| Server::Private { place, code, access: true })
}

/// Where to send a client back to: its private server if it was launched into one, else its last public server.
pub fn server(command_line: Option<&str>, log: Option<&str>) -> Option<Server> {
    command_line.and_then(private_server).or_else(|| log.and_then(last_join).map(|(place, job)| Server::Public { place, job }))
}

/// The file the app writes for an account's helper: an id (so each request runs once), then one join link a line.
pub fn request(id: u64, servers: &[Server]) -> String {
    std::iter::once(id.to_string()).chain(servers.iter().map(Server::link)).collect::<Vec<_>>().join("\n")
}

/// A request file read back: its id and the join links in it. Anything that isn't a Roblox join link is dropped,
/// since the helper opens these.
pub fn read_request(text: &str) -> Option<(u64, Vec<String>)> {
    let mut lines = text.lines();
    let id = lines.next()?.trim().parse().ok()?;
    let links = lines.map(str::trim).filter(|l| l.strip_prefix("roblox://experiences/start?placeId=")
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '&' | '='))))
        .map(String::from).collect();
    Some((id, links))
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The value of `name=` in a query string (letters, digits and dashes only, so nothing else can ride along).
fn param(text: &str, name: &str) -> Option<String> {
    let key = format!("{name}=");
    let start = text.match_indices(&key).find(|(i, _)| *i == 0 || matches!(text.as_bytes()[i - 1], b'?' | b'&' | b' ' | b'"'))?.0 + key.len();
    let value: String = text[start..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    valid_id(&value).then_some(value)
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        // bytes, not str slicing: a command line can hold any text
        if b[i] == b'%' && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "2026-11-13T10:00:01.1Z,1.2,3,0 [FLog::Output] ! Joining game 'aaaaaaaa-1111-2222-3333-444444444444' place 1537690962 at 128.116.1.2\n\
        2026-11-13T10:05:00.0Z,1.2,3,0 [FLog::Network] something else\n\
        2026-11-13T10:20:07.5Z,1.2,3,0 [FLog::Output] ! Joining game 'bbbbbbbb-5555-6666-7777-888888888888' place 4189852503 at 128.116.9.9\n";

    #[test]
    fn follows_the_last_join_in_the_log() {
        assert_eq!(last_join(LOG), Some((4189852503, "bbbbbbbb-5555-6666-7777-888888888888".into())));
        assert_eq!(last_join("nothing here"), None);
        assert_eq!(last_join("! Joining game 'x;rm' place 1 at 1"), None);
    }

    #[test]
    fn private_servers_come_from_the_launch() {
        let cl = r#""C:\Roblox\RobloxPlayerBeta.exe" --app -t SECRET -j "https%3A%2F%2Fassetgame.roblox.com%2Fgame%2FPlaceLauncher.ashx%3Frequest%3DRequestPrivateGame%26placeId%3D1537690962%26accessCode%3Dabc-123""#;
        assert_eq!(private_server(cl), Some(Server::Private { place: 1537690962, code: "abc-123".into(), access: true }));
        let deep = "RobloxPlayerBeta.exe --deeplink roblox://experiences/start?placeId=1537690962&linkCode=987654321";
        assert_eq!(server(Some(deep), Some(LOG)), Some(Server::Private { place: 1537690962, code: "987654321".into(), access: false }));
        // a public launch falls back to the log, so a teleport is followed
        assert_eq!(server(Some("RobloxPlayerBeta.exe --app -t SECRET"), Some(LOG)),
                   Some(Server::Public { place: 4189852503, job: "bbbbbbbb-5555-6666-7777-888888888888".into() }));
        assert_eq!(server(None, None), None);
    }

    #[test]
    fn requests_round_trip_and_only_carry_join_links() {
        let servers = [Server::Public { place: 7, job: "j-1".into() }, Server::Private { place: 8, code: "c".into(), access: false }];
        let (id, links) = read_request(&request(42, &servers)).unwrap();
        assert_eq!(id, 42);
        assert_eq!(links, servers.iter().map(Server::link).collect::<Vec<_>>());
        let evil = "1\nroblox://experiences/start?placeId=1&x=\"cmd /c calc\"\nhttps://evil.example\nC:\\bad.exe";
        assert_eq!(read_request(evil).unwrap().1, Vec::<String>::new());
        assert_eq!(read_request("not a number"), None);
    }

    #[test]
    fn odd_command_lines_dont_crash() {
        assert_eq!(private_server("é%é%2 placeId=%zz ünïcode %"), None);
    }

    #[test]
    fn links_never_carry_the_login_ticket() {
        let s = server(Some("x -t SECRETTICKET placeId=5&linkCode=12"), None).unwrap();
        assert_eq!(s.link(), "roblox://experiences/start?placeId=5&linkCode=12");
        assert_eq!(Server::Public { place: 7, job: "j-1".into() }.link(), "roblox://experiences/start?placeId=7&gameInstanceId=j-1");
    }
}
