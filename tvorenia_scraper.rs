use anyhow::{anyhow, Context, Result};
use clap::Parser;
use encoding_rs::{Encoding, UTF_8, WINDOWS_1251};
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, USER_AGENT};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;
use url::Url;

const DEFAULT_ROOT_URL: &str = "http://tvorenia.russportal.ru/";
const MAIN_BEGIN: &str = "<!--Main Content-->";
const MAIN_END: &str = "<!--EndOf Main Content-->";

#[derive(Debug, Clone)]
struct CrawlScope {
    host: String,
    root_url: Url,
    path_prefix: String,
}

impl CrawlScope {
    fn from_start(start: &Url) -> Result<Self> {
        let host = start
            .host_str()
            .ok_or_else(|| anyhow!("--start URL has no host: {start}"))?
            .to_string();
        if start.scheme() != "http" && start.scheme() != "https" {
            return Err(anyhow!("--start must be http or https: {start}"));
        }
        let root_url = Url::parse(&format!("{}://{}/", start.scheme(), host))?;
        let path_prefix = path_prefix_from_start(start.path());
        Ok(Self { host, root_url, path_prefix })
    }
}

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Сниматель страницъ съ продолженіемъ работы для tvorenia.russportal.ru",
    help_template = "{before-help}{name} {version}\n{about}\n\nУпотребленіе:\n  {usage}\n\nПараметры и доводы:\n{all-args}{after-help}"
)]
struct Args {
    /// Начальный URL. Обыкновенно корень сайта или страница отдѣла.
    #[arg(long, default_value = DEFAULT_ROOT_URL)]
    start: String,

    /// Выходная папка.
    #[arg(long, default_value = "tvorenia_dump")]
    out: PathBuf,

    /// Почтительная задержка между сѣтевыми запросами.
    #[arg(long, default_value_t = 1200)]
    delay_ms: u64,

    /// Остановиться послѣ сего числа новыхъ скачиваній. Не указывать для полнаго обхода.
    #[arg(long)]
    limit: Option<usize>,

    /// Въ семъ запускѣ вновь пробовать неудавшіяся страницы, а не оставлять ихъ въ спискѣ ошибокъ.
    #[arg(long)]
    retry_failed: bool,

    /// Печатать однострочный ходъ работы послѣ каждыхъ N сохраненныхъ/пропущенныхъ страницъ.
    #[arg(long, default_value_t = 1)]
    progress_every: usize,

    /// Печатать образчикъ текста UTF-8 изъ каждой сохраненной страницы. 0 — отключить.
    #[arg(long, default_value_t = 220)]
    sample_chars: usize,

    /// Писать state.json/progress.json не послѣ каждой страницы, а послѣ каждыхъ N обработанныхъ URL. 1 — старое поведеніе.
    #[arg(long, default_value_t = 100)]
    checkpoint_every: usize,

    /// Папка для state.json/progress.json, напр. /dev/shm/scraper_state для уменьшенія дисковаго IO.
    #[arg(long)]
    checkpoint_dir: Option<PathBuf>,

    /// Не сохранять полныя raw/*.html страницы; уменьшаетъ запись на дискъ.
    #[arg(long)]
    no_raw_html: bool,

    /// Не сохранять content_html/*.html; оставить только markdown/*.md.
    #[arg(long)]
    no_content_html: bool,

    /// Скачивать страницы заново, даже если состояніе/файлы говорятъ, что онѣ уже сохранены.
    #[arg(long)]
    redownload: bool,

    /// Не пробовать URL, уже внесенныя въ списокъ неудачъ въ state.json.
    #[arg(long)]
    skip_failed: bool,

    /// Исключать URL, совпадающіе съ regex. Можно указывать нѣсколько разъ.
    #[arg(long = "exclude-url-regex")]
    exclude_url_regex: Vec<String>,

    /// HTTP-статусы, кои надлежитъ пропускать и не считать неудачами. Пусто — отключить.
    #[arg(long, default_value = "403")]
    skip_http_status: String,

    /// Не скачивать, а пересобрать markdown из уже имеющихся raw/*.html (или raw/*.html.zst).
    /// Использует текущую исправленную версию конвертера html→markdown.
    /// Полезно для дамповъ, скачанных предыдущей (с багом) версіей.
    /// Не трогает raw/ и content_html/.
    #[arg(long)]
    fix_markdown: bool,

    /// Сохранять markdown какъ отдѣльные сжатые файлы `markdown/*.md.zst` (вмѣсто обычныхъ .md).
    /// Значительно уменьшаетъ размѣръ и количество записей на дискъ.
    /// Поддерживаетъ уровни 19 и 21 съ --zstd-ultra.
    #[arg(long)]
    zstd: bool,

    /// Уровень сжатія zstd (19 по умолчанію; 21 для максимума).
    #[arg(long, default_value_t = 19)]
    zstd_level: i32,

    /// Включить ultra-режимъ (для level 21 — максимальное сжатіе, требуетъ много памяти).
    #[arg(long)]
    zstd_ultra: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct State {
    queue: VecDeque<Url>,
    seen: HashSet<Url>,
    downloaded: HashSet<Url>,
    failed: HashMap<Url, String>,
    skipped: HashMap<Url, String>,
}

#[derive(Debug, Serialize)]
struct ManifestRecord<'a> {
    url: &'a Url,
    title: String,
    file_stem: String,
    content_html: String,
    markdown: String,
    raw_html: String,
    links_found: usize,
}

#[derive(Debug)]
struct SaveResult {
    links: Vec<Url>,
    markdown: String,
    content_html: String,
    raw_html: String,
    text_chars: usize,
    sample: String,
}

#[derive(Debug, Serialize)]
struct ProgressSnapshot<'a> {
    downloaded: usize,
    queued: usize,
    seen: usize,
    failed: usize,
    skipped: usize,
    frontier_total: usize,
    frontier_percent: f64,
    last_url: Option<&'a Url>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.fix_markdown {
        return fix_markdown_from_raw(&args);
    }
    fs::create_dir_all(args.out.join("raw"))?;
    fs::create_dir_all(args.out.join("content_html"))?;
    fs::create_dir_all(args.out.join("markdown"))?;
    fs::create_dir_all(checkpoint_root(&args))?;

    let start_url = Url::parse(&args.start).context("invalid --start URL")?;
    let scope = CrawlScope::from_start(&start_url)?;
    let mut state = load_or_seed_state(&args, &scope)?;
    if args.retry_failed {
        let failed_urls: Vec<Url> = state.failed.keys().cloned().collect();
        for url in failed_urls {
            if !state.downloaded.contains(&url) && !state.queue.contains(&url) {
                state.queue.push_back(url.clone());
            }
            state.failed.remove(&url);
        }
        save_state(checkpoint_root(&args), &state)?;
    }

    let client = build_client()?;
    let skip_statuses = parse_status_list(&args.skip_http_status)?;
    let exclude_url_regexes = compile_url_regexes(&args.exclude_url_regex)?;
    let mut new_downloads = 0usize;
    let mut processed_this_run = 0usize;

    println!(
        "Checkpoint loaded: downloaded={}, queue={}, seen={}, failed={}, skipped={}, out={}",
        state.downloaded.len(),
        state.queue.len(),
        state.seen.len(),
        state.failed.len(),
        state.skipped.len(),
        args.out.display()
    );

    while let Some(url) = state.queue.pop_front() {
        if let Some(pattern) = excluded_by(&url, &exclude_url_regexes) {
            let reason = format!("excluded by --exclude-url-regex: {pattern}");
            eprintln!("  SKIP: {reason} for {url}");
            state.skipped.insert(url.clone(), reason);
            state.failed.remove(&url);
            processed_this_run += 1;
            maybe_checkpoint(&args, &state, Some(&url), processed_this_run)?;
            continue;
        }

        if !args.redownload && (state.skipped.contains_key(&url) || (args.skip_failed && state.failed.contains_key(&url))) {
            processed_this_run += 1;
            maybe_checkpoint(&args, &state, Some(&url), processed_this_run)?;
            if should_print_progress(args.progress_every, processed_this_run) {
                println!("  skipped known bad URL; {}", progress_line(&state));
            }
            continue;
        }

        if !args.redownload && (state.downloaded.contains(&url) || page_already_saved(&args.out, &url, &args)) {
            // If files exist but state.json was written before link discovery
            // (for example after a crash), re-discover links from the local raw
            // HTML instead of re-downloading the page.
            if !state.downloaded.contains(&url) {
                if let Ok(html) = read_saved_raw_html(&args.out, &url) {
                    for link in discover_links(&html, &url, &scope, &exclude_url_regexes)? {
                        if !state.seen.contains(&link) && !state.downloaded.contains(&link) {
                            state.seen.insert(link.clone());
                            state.queue.push_back(link);
                        }
                    }
                }
            }
            state.downloaded.insert(url.clone());
            processed_this_run += 1;
            maybe_checkpoint(&args, &state, Some(&url), processed_this_run)?;
            if should_print_progress(args.progress_every, processed_this_run) {
                println!("  skipped existing local files; {}", progress_line(&state));
            }
            continue;
        }

        if let Some(limit) = args.limit {
            if new_downloads >= limit {
                state.queue.push_front(url);
                save_state(checkpoint_root(&args), &state)?;
                println!("Reached --limit={limit}. Checkpoint saved; run again to continue.");
                return Ok(());
            }
        }

        println!("{} GET {url}", progress_line(&state));
        match fetch_page(&client, &url) {
            Ok(html) => {
                let result = save_page_and_discover_links(
                    &args.out,
                    &url,
                    &html,
                    args.sample_chars,
                    &scope,
                    &exclude_url_regexes,
                    &args,
                )
                .with_context(|| format!("saving {url}"));
                match result {
                    Ok(saved) => {
                        for link in &saved.links {
                            if !state.seen.contains(link) && !state.downloaded.contains(link) {
                                state.seen.insert(link.clone());
                                state.queue.push_back(link.clone());
                            }
                        }
                        state.downloaded.insert(url.clone());
                        state.failed.remove(&url);
                        state.skipped.remove(&url);
                        new_downloads += 1;
                        processed_this_run += 1;
                        println!(
                            "  saved: md={}, html={}, raw={}, text_chars={}, discovered={}",
                            saved.markdown,
                            saved.content_html,
                            saved.raw_html,
                            saved.text_chars,
                            saved.links.len()
                        );
                        if args.sample_chars > 0 && !saved.sample.is_empty() {
                            println!("  sample UTF-8: {}", saved.sample);
                        }
                        if should_print_progress(args.progress_every, processed_this_run) {
                            println!("  {}", progress_line(&state));
                        }
                    }
                    Err(e) => {
                        eprintln!("  ERROR: {e:#}");
                        state.failed.insert(url.clone(), format!("{e:#}"));
                        processed_this_run += 1;
                    }
                }
            }
            Err(e) => {
                if let FetchError::HttpStatus { status, .. } = &e {
                    if skip_statuses.contains(status) {
                        let reason = format!("HTTP {status} skipped by --skip-http-status");
                        eprintln!("  SKIP: {reason} for {url}");
                        state.skipped.insert(url.clone(), reason);
                        state.failed.remove(&url);
                        processed_this_run += 1;
                        maybe_checkpoint(&args, &state, Some(&url), processed_this_run)?;
                        sleep(Duration::from_millis(args.delay_ms));
                        continue;
                    }
                }
                eprintln!("  ERROR: {e:#}");
                state.failed.insert(url.clone(), format!("{e:#}"));
                processed_this_run += 1;
            }
        }

        // Checkpoint is intentionally throttled: state.json can be huge.
        maybe_checkpoint(&args, &state, Some(&url), processed_this_run)?;
        sleep(Duration::from_millis(args.delay_ms));
    }

    save_state(checkpoint_root(&args), &state)?;
    save_progress(checkpoint_root(&args), &state, None)?;

    println!(
        "Done. downloaded={}, failed={}, skipped={}. Output: {}",
        state.downloaded.len(),
        state.failed.len(),
        state.skipped.len(),
        args.out.display()
    );
    Ok(())
}

fn build_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Mozilla/5.0 (compatible; tvorenia-archiver/0.1; +offline personal archive)"),
    );
    headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("ru,en;q=0.8"));

    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .context("building HTTP client")
}

fn checkpoint_root(args: &Args) -> &Path {
    args.checkpoint_dir.as_deref().unwrap_or(&args.out)
}

fn load_or_seed_state(args: &Args, scope: &CrawlScope) -> Result<State> {
    let path = checkpoint_root(args).join("state.json");
    if path.exists() {
        let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let state: State = serde_json::from_slice(&bytes).context("parsing state.json")?;
        ensure_state_matches_scope(&state, scope, &path)?;
        return Ok(state);
    }

    let parsed_start = Url::parse(&args.start).context("invalid --start URL")?;
    let start = normalize_url(&parsed_start, scope)
        .ok_or_else(|| anyhow!("--start is not a crawlable HTML page on {}: {}", scope.host, args.start))?;
    let mut state = State::default();
    state.seen.insert(start.clone());
    state.queue.push_back(start);
    save_state(checkpoint_root(&args), &state)?;
    Ok(state)
}

fn compile_url_regexes(patterns: &[String]) -> Result<Vec<(String, Regex)>> {
    patterns
        .iter()
        .map(|p| {
            Regex::new(p)
                .map(|re| (p.clone(), re))
                .with_context(|| format!("bad regex in --exclude-url-regex: {p}"))
        })
        .collect()
}

fn excluded_by(url: &Url, regexes: &[(String, Regex)]) -> Option<String> {
    let s = url.as_str();
    regexes
        .iter()
        .find(|(_, re)| re.is_match(s))
        .map(|(pattern, _)| pattern.clone())
}

fn parse_status_list(input: &str) -> Result<HashSet<u16>> {
    let mut out = HashSet::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let code: u16 = part
            .parse()
            .with_context(|| format!("bad HTTP status in --skip-http-status: {part}"))?;
        out.insert(code);
    }
    Ok(out)
}

fn ensure_state_matches_scope(state: &State, scope: &CrawlScope, path: &Path) -> Result<()> {
    let first_other_host = state
        .queue
        .iter()
        .chain(state.seen.iter())
        .chain(state.downloaded.iter())
        .chain(state.failed.keys())
        .chain(state.skipped.keys())
        .filter_map(|u| u.host_str().map(|h| (u, h)))
        .find(|(_, h)| *h != scope.host);

    if let Some((url, host)) = first_other_host {
        return Err(anyhow!(
            "checkpoint {} belongs to host {}, but --start is on host {}. Use a different --out directory or remove the old state.json. First mismatching URL: {}",
            path.display(),
            host,
            scope.host,
            url
        ));
    }
    Ok(())
}

#[derive(Debug)]
enum FetchError {
    HttpStatus { status: u16, url: Url },
    Other(anyhow::Error),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::HttpStatus { status, url } => write!(f, "HTTP {status} for {url}"),
            FetchError::Other(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for FetchError {}

fn fetch_page(client: &Client, url: &Url) -> std::result::Result<String, FetchError> {
    let resp = client
        .get(url.clone())
        .send()
        .map_err(|e| FetchError::Other(anyhow!(e).context(format!("request failed: {url}"))))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(FetchError::HttpStatus {
            status: status.as_u16(),
            url: url.clone(),
        });
    }
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp
        .bytes()
        .map_err(|e| FetchError::Other(anyhow!(e).context("reading response body")))?;

    let (text, encoding_name, had_errors) = decode_html_bytes(&bytes, content_type.as_deref());
    if had_errors {
        eprintln!("  warning: had {encoding_name} decode errors in {url}");
    }
    Ok(text)
}

fn decode_html_bytes(bytes: &[u8], content_type: Option<&str>) -> (String, String, bool) {
    let encoding = encoding_from_content_type(content_type)
        .or_else(|| encoding_from_meta_charset(bytes))
        .unwrap_or_else(|| {
            if std::str::from_utf8(bytes).is_ok() {
                UTF_8
            } else {
                WINDOWS_1251
            }
        });

    let (text, _, had_errors) = encoding.decode(bytes);
    (text.into_owned(), encoding.name().to_string(), had_errors)
}

fn encoding_from_content_type(content_type: Option<&str>) -> Option<&'static Encoding> {
    let content_type = content_type?;
    let lower = content_type.to_ascii_lowercase();
    let pos = lower.find("charset=")?;
    let label = content_type[pos + "charset=".len()..]
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c| c == '\"' || c == '\'');
    Encoding::for_label(label.as_bytes())
}

fn encoding_from_meta_charset(bytes: &[u8]) -> Option<&'static Encoding> {
    let prefix_len = bytes.len().min(8192);
    let prefix = String::from_utf8_lossy(&bytes[..prefix_len]).to_ascii_lowercase();
    for needle in ["charset=", "charset ="] {
        if let Some(pos) = prefix.find(needle) {
            let after = &prefix[pos + needle.len()..];
            let label = after
                .trim_start_matches(|c| c == ' ' || c == '\t' || c == '\'' || c == '\"')
                .split(|c: char| c == '\"' || c == '\'' || c == '>' || c == ';' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .trim();
            if let Some(enc) = Encoding::for_label(label.as_bytes()) {
                return Some(enc);
            }
        }
    }
    None
}

fn save_page_and_discover_links(
    out: &Path,
    url: &Url,
    html: &str,
    sample_chars: usize,
    scope: &CrawlScope,
    exclude_url_regexes: &[(String, Regex)],
    args: &Args,
) -> Result<SaveResult> {
    let stem = file_stem_for_url(url);
    let title = extract_title(html).unwrap_or_else(|| stem.clone());
    let content = extract_main_content(html).unwrap_or(html);

    let raw_rel = format!("raw/{stem}.html");
    let content_rel = format!("content_html/{stem}.html");
    let md_rel = format!("markdown/{stem}.md");

    if !args.no_raw_html {
        atomic_write(out.join(&raw_rel), html.as_bytes())?;
    }
    if !args.no_content_html {
        atomic_write(out.join(&content_rel), content.as_bytes())?;
    }
    let markdown = html_to_markdownish(content, url, &title)?;
    let plain_text = markdown_plain_body(&markdown);
    let text_chars = plain_text.chars().count();
    let sample = make_sample(&plain_text, sample_chars);

    if args.zstd {
        // Per-file compressed saving: markdown/{stem}.md.zst
        // This is the goal — dramatically reduce size and disk writes vs plain text.
        let zstd_rel = format!("{}.zst", md_rel); // e.g. markdown/xxx.md.zst
        let compressed = zstd::encode_all(markdown.as_bytes(), args.zstd_level)
            .with_context(|| format!("zstd compressing {}", zstd_rel))?;
        atomic_write(out.join(&zstd_rel), &compressed)?;
    } else {
        // Normal plain text .md
        atomic_write(out.join(&md_rel), markdown.as_bytes())?;
    }

    let links = discover_links(html, url, scope, exclude_url_regexes)?;

    let reported_md = if args.zstd {
        format!("{}.zst", md_rel)
    } else {
        md_rel.clone()
    };

    let record = ManifestRecord {
        url,
        title,
        file_stem: stem,
        content_html: if args.no_content_html { String::new() } else { content_rel.clone() },
        markdown: reported_md.clone(),
        raw_html: if args.no_raw_html { String::new() } else { raw_rel.clone() },
        links_found: links.len(),
    };
    append_manifest(out.join("manifest.jsonl"), &record)?;
    Ok(SaveResult {
        links,
        markdown: reported_md,
        content_html: if args.no_content_html { "<disabled>".to_string() } else { content_rel },
        raw_html: if args.no_raw_html { "<disabled>".to_string() } else { raw_rel },
        text_chars,
        sample,
    })
}

fn discover_links(
    html: &str,
    base: &Url,
    scope: &CrawlScope,
    exclude_url_regexes: &[(String, Regex)],
) -> Result<Vec<Url>> {
    let doc = Html::parse_document(html);
    let selector = Selector::parse("a[href]").expect("valid selector");
    let mut links = Vec::new();
    let mut unique = HashSet::new();

    for a in doc.select(&selector) {
        let Some(href) = a.value().attr("href") else { continue };
        if href.starts_with("#") || href.starts_with("mailto:") || href.starts_with("javascript:") {
            continue;
        }
        let Ok(joined) = base.join(href) else { continue };
        let Some(norm) = normalize_url(&joined, scope) else { continue };
        if excluded_by(&norm, exclude_url_regexes).is_some() {
            continue;
        }
        if unique.insert(norm.clone()) {
            links.push(norm);
        }
    }
    Ok(links)
}

fn normalize_url(url: &Url, scope: &CrawlScope) -> Option<Url> {
    let host = url.host_str()?;
    if host != scope.host {
        return None;
    }
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }

    let path = if url.path().is_empty() { "/" } else { url.path() };
    if !path_is_inside_prefix(path, &scope.path_prefix) {
        return None;
    }
    if is_probably_asset_path(path) {
        return None;
    }

    let mut out = scope.root_url.clone();
    out.set_path(path);
    out.set_fragment(None);

    // RussPortal index.php pages use only the `id` query parameter for document identity.
    // For other sites (e.g. azbyka.ru/otechnik) keep non-empty queries but drop fragments.
    if path.ends_with("/index.php") || path == "/index.php" || path == "index.php" {
        let id = url
            .query_pairs()
            .find(|(k, _)| k == "id")
            .map(|(_, v)| v.into_owned());
        match id {
            Some(id) if !id.trim().is_empty() => out.set_query(Some(&format!("id={}", id))),
            _ => out.set_query(None),
        }
    } else {
        out.set_query(url.query());
    }
    Some(out)
}

fn path_prefix_from_start(path: &str) -> String {
    let path = if path.is_empty() { "/" } else { path };
    if path == "/" {
        return "/".to_string();
    }
    path.trim_end_matches('/').to_string()
}

fn path_is_inside_prefix(path: &str, prefix: &str) -> bool {
    if prefix == "/" {
        return true;
    }
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

fn is_probably_asset_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".ico", ".css", ".js",
        ".mjs", ".pdf", ".djvu", ".zip", ".rar", ".7z", ".tar", ".gz", ".mp3", ".mp4",
        ".avi", ".mov", ".webm", ".woff", ".woff2", ".ttf", ".eot",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

fn extract_title(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    for selector_text in ["h1", "title"] {
        let selector = Selector::parse(selector_text).ok()?;
        if let Some(el) = doc.select(&selector).next() {
            let text = el.text().collect::<Vec<_>>().join(" ");
            let text = clean_ws(&html_escape::decode_html_entities(&text));
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn extract_main_content(html: &str) -> Option<&str> {
    let start = html.find(MAIN_BEGIN)? + MAIN_BEGIN.len();
    let tail = &html[start..];
    let end = tail.find(MAIN_END)?;
    Some(&tail[..end])
}

fn html_to_markdownish(html: &str, url: &Url, title: &str) -> Result<String> {
    let mut s = html.to_string();

    // Remove noisy blocks first.
    for pat in [
        r"(?is)<!--.*?-->",
        r"(?is)<script\b[^>]*>.*?</script>",
        r"(?is)<style\b[^>]*>.*?</style>",
        r"(?is)<form\b[^>]*>.*?</form>",
        r"(?is)<iframe\b[^>]*>.*?</iframe>",
    ] {
        s = Regex::new(pat)?.replace_all(&s, "").into_owned();
    }

    let replacements = [
        (r"(?i)<\s*h1\b[^>]*>", "\n# "),
        (r"(?i)<\s*/\s*h1\s*>", "\n\n"),
        (r"(?i)<\s*h2\b[^>]*>", "\n## "),
        (r"(?i)<\s*/\s*h2\s*>", "\n\n"),
        (r"(?i)<\s*h3\b[^>]*>", "\n### "),
        (r"(?i)<\s*/\s*h3\s*>", "\n\n"),
        (r"(?i)<\s*br\b[^>]*>", "\n"),
        (r"(?i)<\s*/\s*p\s*>", "\n\n"),
        (r"(?i)<\s*p\b[^>]*>", "\n\n"),
        (r"(?i)<\s*/\s*div\s*>", "\n"),
        (r"(?i)<\s*/\s*center\s*>", "\n"),
        (r"(?i)<\s*li\b[^>]*>", "\n- "),
        (r"(?i)</\s*tr\s*>", "\n"),
        (r"(?i)</\s*td\s*>", " | "),
    ];
    for (pat, repl) in replacements {
        s = Regex::new(pat)?.replace_all(&s, repl).into_owned();
    }

    // Drop remaining tags. The companion content_html file preserves original markup.
    // Replace with space (rather than "") so that inline tags wrapping adjacent words
    // (common in old HTML) do not glue words together if source had no ws between tags.
    // Later punctuation cleanup removes unwanted spaces before . , etc.
    s = Regex::new(r"(?is)<[^>]+>")?.replace_all(&s, " ").into_owned();
    s = html_escape::decode_html_entities(&s).into_owned();
    s = s.replace('\r', "\n");
    s = Regex::new(r"[ \t]+")?.replace_all(&s, " ").into_owned();
    s = Regex::new(r"[\u{00A0}\u{202F}]+")?.replace_all(&s, " ").into_owned();
    s = Regex::new(r"\n[ \t]+")?.replace_all(&s, "\n").into_owned();
    s = Regex::new(r"\n{3,}")?.replace_all(&s, "\n\n").into_owned();
    let mut body = derazryadka_text(s.trim());

    // Glue first letter that was in its own tag (very common for drop caps / emphasized initials).
    // This repairs "В ъ" → "Въ", "О коло" → "Около" etc. caused by tag→" " stripping.
    // Only capital + lowercase to be conservative. Use explicit ws to handle nbsp too.
    body = Regex::new(r"\b([А-ЯіІѢѲѴЁ])[ \t\u{00A0}\u{202F}]+([а-яіѣѳѵъьё])")?
        .replace_all(&body, "$1$2")
        .into_owned();

    // Cleanup spaces that may have been inserted by tag->space replacement,
    // or that ended up before/after punctuation.
    body = Regex::new(r" ([.,;:!?)\]»”])")?.replace_all(&body, "$1").into_owned();
    body = Regex::new(r"([(\[«“]) ")?.replace_all(&body, "$1").into_owned();

    Ok(format!("# {title}\n\nSource: {url}\n\n---\n\n{body}\n"))
}

fn file_stem_for_url(url: &Url) -> String {
    if let Some(id) = url
        .query_pairs()
        .find(|(k, _)| k == "id")
        .map(|(_, v)| v.into_owned())
        .filter(|id| !id.trim().is_empty())
    {
        return sanitize_stem(&id);
    }

    let mut key = url.path().trim_matches('/').replace('/', ".");
    if key.is_empty() {
        key = "index".to_string();
    }
    if let Some(query) = url.query() {
        if !query.is_empty() {
            key.push_str("__q_");
            key.push_str(query);
        }
    }
    sanitize_stem(&key)
}

fn sanitize_stem(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    while out.contains("..") {
        out = out.replace("..", ".");
    }
    let out = out.trim_matches(['.', '_', '-']);
    if out.is_empty() {
        "page".to_string()
    } else {
        out.to_string()
    }
}

fn page_already_saved(out: &Path, url: &Url, args: &Args) -> bool {
    let stem = file_stem_for_url(url);
    let md_dir = out.join("markdown");
    let markdown_ok = if args.zstd {
        md_dir.join(format!("{stem}.md.zst")).exists()
    } else {
        md_dir.join(format!("{stem}.md")).exists()
    };
    let content_ok = args.no_content_html || out.join("content_html").join(format!("{stem}.html")).exists();
    let raw_ok = args.no_raw_html || out.join("raw").join(format!("{stem}.html")).exists();
    markdown_ok && content_ok && raw_ok
}

fn read_saved_raw_html(out: &Path, url: &Url) -> Result<String> {
    let stem = file_stem_for_url(url);
    let path = out.join("raw").join(format!("{stem}.html"));
    fs::read_to_string(&path).with_context(|| format!("reading saved raw HTML {}", path.display()))
}

fn should_print_progress(progress_every: usize, processed_this_run: usize) -> bool {
    progress_every > 0 && processed_this_run % progress_every == 0
}

fn progress_numbers(state: &State) -> (usize, usize, usize, usize, usize, f64) {
    let downloaded = state.downloaded.len();
    let queued = state.queue.len();
    let seen = state.seen.len();
    let failed = state.failed.len();
    // `seen` is the best known total so far. It grows as section pages reveal more links,
    // so this is a frontier/known-URLs percentage, not a final site-wide percentage.
    let frontier_total = seen.max(downloaded + queued);
    let frontier_percent = if frontier_total == 0 {
        100.0
    } else {
        (downloaded as f64 * 100.0) / frontier_total as f64
    };
    (downloaded, queued, seen, failed, frontier_total, frontier_percent)
}

fn progress_line(state: &State) -> String {
    let (downloaded, queued, seen, failed, frontier_total, frontier_percent) = progress_numbers(state);
    format!(
        "progress: {downloaded}/{frontier_total} frontier ({frontier_percent:.2}%), queue={queued}, seen={seen}, failed={failed}, skipped={}",
        state.skipped.len()
    )
}

fn save_progress(out: &Path, state: &State, last_url: Option<&Url>) -> Result<()> {
    let (downloaded, queued, seen, failed, frontier_total, frontier_percent) = progress_numbers(state);
    let snapshot = ProgressSnapshot {
        downloaded,
        queued,
        seen,
        failed,
        skipped: state.skipped.len(),
        frontier_total,
        frontier_percent,
        last_url,
    };
    let data = serde_json::to_vec_pretty(&snapshot).context("serializing progress")?;
    atomic_write(out.join("progress.json"), &data)
}

fn markdown_plain_body(markdown: &str) -> String {
    // Keep the human text but remove the metadata header used by this scraper.
    let body = markdown.split_once("---").map(|(_, b)| b).unwrap_or(markdown);
    clean_ws(body)
}

fn make_sample(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let mut sample = text.chars().take(limit).collect::<String>();
    if text.chars().count() > limit {
        sample.push('…');
    }
    sample
}

fn derazryadka_text(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < chars.len() {
        if let Some(parsed) = parse_razryadka_phrase(&chars, i) {
            out.push_str(&parsed.text);
            i = parsed.end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

struct RazryadkaParsed {
    text: String,
    end: usize,
}

fn parse_razryadka_phrase(chars: &[char], start: usize) -> Option<RazryadkaParsed> {
    let (first, mut i) = read_letter_cluster(chars, start)?;
    // Razryadka phrase can only start at the beginning of a "word" (after space/punct or start).
    // If previous char is alphabetic, this is mid-word (e.g. last letter of normal word + space + next),
    // reject to avoid chaining last-letter into following razryadka or connector (generalizes the
    // original "X и Y" false positive).
    if start > 0 && chars[start - 1].is_alphabetic() {
        return None;
    }
    let start_lower = first.to_lowercase();
    let mut words: Vec<String> = Vec::new();
    let mut current = first;
    let mut single_sep_count = 0usize;
    let mut word_break_count = 0usize;
    let mut total_letters = 1usize;
    let mut max_word_letters = 1usize;
    let mut current_word_letters = 1usize;

    loop {
        let sep_start = i;
        while i < chars.len() && is_razryadka_space(chars[i]) {
            i += 1;
        }
        let sep_len = i - sep_start;
        if sep_len == 0 {
            break;
        }

        let Some((next, next_i)) = read_letter_cluster(chars, i) else {
            i = sep_start;
            break;
        };

        if sep_len == 1 {
            // Only append if this appears to be a single-letter cluster (as in razryadka),
            // i.e. the char immediately after this cluster is NOT alphabetic (would mean
            // start of a normal multi-letter word, in which case we stop the raz phrase
            // before including its first letter, to avoid gluing e.g. first letter of
            // following word after raz, or in connector+raz cases).
            let is_start_of_longer_word = next_i < chars.len() && chars[next_i].is_alphabetic();
            if is_start_of_longer_word {
                i = sep_start;
                break;
            }
            current.push_str(&next);
            current_word_letters += 1;
            total_letters += 1;
            single_sep_count += 1;
            max_word_letters = max_word_letters.max(current_word_letters);
        } else {
            words.push(current);
            current = next;
            current_word_letters = 1;
            total_letters += 1;
            word_break_count += 1;
        }
        i = next_i;
    }

    max_word_letters = max_word_letters.max(current_word_letters);
    words.push(current);

    // Require at least a real spaced word: two or more single separators, e.g. р а д.
    // We require max_word_letters >=4 (i.e. at least 3 single-seps / 4 clusters) for
    // pure single-sep phrases to avoid false positives on normal text such as
    // "X и Y" (last letter of prev word + "и" + first letter of next word looks like
    // 3-cluster razryadka). "И русскій" or "а въ" are already rejected by <2 seps.
    // Two-letter words like "к ъ" are fixed when part of larger spaced phrase.
    if single_sep_count < 2 {
        return None;
    }

    // Avoid gluing common single-letter words ("и", "о") into a following
    // razryadka phrase. E.g. "и о зарытыхъ" (with raz on "зарытыхъ"), "литературой о морскихъ",
    // "Прекрасенъ и самый" etc. would otherwise become "иозарытыхъ", "литературойоморскихъ",
    // "Прекрасенъисамый" because the detector chains across the normal space.
    // Real razryadka that happens to start with "и" or "о" will instead get its first letter
    // left separated (e.g. "и стина" instead of "истина"), which is a lesser evil than
    // incorrect word gluing.
    if start_lower.chars().count() == 1 {
        let ch = start_lower.chars().next().unwrap();
        if ch == 'и' || ch == 'о' {
            return None;
        }
    }

    if max_word_letters < 4 && !(word_break_count > 0 && total_letters >= 5) {
        return None;
    }

    Some(RazryadkaParsed {
        text: words.join(" "),
        end: i,
    })
}

fn read_letter_cluster(chars: &[char], start: usize) -> Option<(String, usize)> {
    let first = *chars.get(start)?;
    if !first.is_alphabetic() {
        return None;
    }
    let mut s = String::new();
    s.push(first);
    let mut i = start + 1;
    while i < chars.len() && is_combining_mark(chars[i]) {
        s.push(chars[i]);
        i += 1;
    }
    Some((s, i))
}

fn is_combining_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

fn is_razryadka_space(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\u{00A0}' || c == '\u{202F}'
}

fn checkpoint_due(every: usize, processed: usize) -> bool {
    every <= 1 || processed % every == 0
}

fn maybe_checkpoint(args: &Args, state: &State, last_url: Option<&Url>, processed: usize) -> Result<()> {
    if checkpoint_due(args.checkpoint_every, processed) {
        save_state(checkpoint_root(args), state)?;
        save_progress(checkpoint_root(args), state, last_url)?;
    }
    Ok(())
}

fn save_state(out: &Path, state: &State) -> Result<()> {
    // Compact JSON: state can contain tens/hundreds of thousands of URLs; pretty JSON
    // caused massive repeated disk writes when checkpointing often.
    let data = serde_json::to_vec(state).context("serializing state")?;
    atomic_write(out.join("state.json"), &data)
}

fn append_manifest(path: PathBuf, record: &ManifestRecord<'_>) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    serde_json::to_writer(&mut f, record).context("writing manifest record")?;
    writeln!(f)?;
    f.flush()?;
    Ok(())
}

fn atomic_write(path: PathBuf, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|s| s.to_str()).unwrap_or("file")
    ));
    fs::write(&tmp, bytes).with_context(|| format!("writing temp file {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| {
        format!(
            "renaming temp file {} -> {}",
            tmp.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn clean_ws(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Режим --fix-markdown: пересобирает markdown (в т.ч. .md.zst при --zstd) из существующих raw/*.html или raw/*.html.zst
/// используя исправленный конвертер (без сети и без изменения raw/content_html).
/// Поддерживает --out, --limit, --progress-every, --sample-chars.
fn fix_markdown_from_raw(args: &Args) -> Result<()> {
    let out = &args.out;
    fs::create_dir_all(out.join("markdown"))?;

    let raw_dir = out.join("raw");
    if !raw_dir.exists() {
        return Err(anyhow!("raw/ directory not found in {} (nothing to convert)", out.display()));
    }

    let mut entries: Vec<_> = fs::read_dir(&raw_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            let s = p.to_string_lossy().to_lowercase();
            s.ends_with(".html") || s.ends_with(".html.zst")
        })
        .collect();
    entries.sort_by_key(|e| e.path().file_name().unwrap_or_default().to_os_string());

    let total = entries.len();
    if total == 0 {
        println!("No raw/*.html or raw/*.html.zst files found in {}", raw_dir.display());
        return Ok(());
    }

    println!(
        "Fixing markdown from {} raw files (html / html.zst) using current fixed converter in {}",
        total,
        out.display()
    );

    let mut processed = 0usize;

    for entry in entries {
        if let Some(limit) = args.limit {
            if processed >= limit {
                break;
            }
        }

        let path = entry.path();
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

        let stem = if file_name.ends_with(".html.zst") {
            file_name.strip_suffix(".html.zst").unwrap_or(&file_name).to_string()
        } else if file_name.ends_with(".html") {
            file_name.strip_suffix(".html").unwrap_or(&file_name).to_string()
        } else {
            file_name.clone()
        };

        println!("  {} / {} : {}", processed + 1, total, stem);

        let bytes = fs::read(&path)
            .with_context(|| format!("reading raw {}", path.display()))?;

        let html = if file_name.ends_with(".zst") {
            let decompressed = zstd::decode_all(&bytes[..])
                .with_context(|| format!("decompressing zstd raw {}", path.display()))?;
            String::from_utf8_lossy(&decompressed).to_string()
        } else {
            String::from_utf8_lossy(&bytes).to_string()
        };

        let title = extract_title(&html).unwrap_or_else(|| stem.clone());
        let content = extract_main_content(&html).unwrap_or(&html);

        // Dummy URL for the "Source:" line in the markdown header.
        // (The original remote URL is not stored in raw files, but the local raw path is.)
        let dummy_url = Url::parse(&format!(
            "http://tvorenia.russportal.ru/raw/{}.html",
            stem
        ))
        .unwrap_or_else(|_| Url::parse("http://tvorenia.russportal.ru/").unwrap());

        let markdown = html_to_markdownish(content, &dummy_url, &title)?;

        if args.zstd {
            let zstd_rel = format!("markdown/{}.md.zst", stem);
            let compressed = zstd::encode_all(markdown.as_bytes(), args.zstd_level)
                .with_context(|| format!("zstd compressing {}", zstd_rel))?;
            atomic_write(out.join(&zstd_rel), &compressed)?;
        } else {
            let md_rel = format!("markdown/{}.md", stem);
            atomic_write(out.join(&md_rel), markdown.as_bytes())?;
        }

        processed += 1;

        if args.sample_chars > 0 {
            let plain_text = markdown_plain_body(&markdown);
            let sample = make_sample(&plain_text, args.sample_chars);
            if !sample.is_empty() {
                println!("  sample UTF-8: {}", sample);
            }
        }

        if should_print_progress(args.progress_every, processed) {
            println!("  progress: {}/{} fixed", processed, total);
        }
    }

    println!(
        "Done. Regenerated {} markdown files from raw (html / html.zst).",
        processed
    );
    Ok(())
}
