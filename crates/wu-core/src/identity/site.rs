//! A program's website, as far as a `.url` shortcut can be said to be to it.

/// Hosts that serve many unrelated projects and people: a link there says
/// nothing about one program unless it also names the path.
const MULTI_TENANT: &[&str] = &[
    "github.com",
    "gitlab.com",
    "bitbucket.org",
    "sourceforge.net",
    "github.io",
    "gitlab.io",
    "netlify.app",
    "vercel.app",
    "pages.dev",
    "herokuapp.com",
    "blogspot.com",
    "wordpress.com",
    "google.com",
    "microsoft.com",
    "apple.com",
    "facebook.com",
    "twitter.com",
    "x.com",
    "youtube.com",
    "linkedin.com",
    "discord.com",
    "discord.gg",
    "t.me",
    "reddit.com",
];

/// `https://www.Acme.example/Widget/?a=1#x` → `acme.example/widget`: host
/// and path only, so http/https, `www.`, case and a trailing slash agree.
pub fn site_key(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next()?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .trim_start_matches("www.")
        .to_lowercase();
    if !host.contains('.') || host.contains(' ') {
        return None;
    }
    let path = path.trim_matches('/').to_lowercase();
    Some(if path.is_empty() {
        host
    } else {
        format!("{host}/{path}")
    })
}

/// True when `url` lies on the program's own site `site` (both from
/// [`site_key`]): the same address or below it, at a path boundary. A site
/// that is only a shared host (`github.com`) matches nothing.
pub fn on_site(site: &str, url: &str) -> bool {
    let Some(url) = site_key(url) else {
        return false;
    };
    let host = site.split('/').next().unwrap_or(site);
    let shared = MULTI_TENANT
        .iter()
        .any(|tenant| host == *tenant || host.ends_with(&format!(".{tenant}")));
    if shared && !site.contains('/') {
        return false;
    }
    url == site
        || url
            .strip_prefix(site)
            .is_some_and(|rest| rest.starts_with('/'))
}
