use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use clap::Parser;
use image::{ImageReader, RgbImage};
use jpegxl_rs::encode::EncoderSpeed;
use jpegxl_rs::encoder_builder;
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, REFERER, USER_AGENT};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;
use url::Url;

const DEFAULT_REFERER: &str = "https://elib.rgo.ru/";
/// Маркеръ въ URL, по коему страница распознаётся какъ задача IIIF (diva/IIPImage).
const IIIF_MARK: &str = "IIIF=";
const INFO_SUFFIX: &str = "/info.json";

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Сниматель сканированныхъ книгъ (diva/IIPImage: elib.rgo.ru, prlib.ru) въ JPEG XL",
    help_template = "{before-help}{name} {version}\n{about}\n\nУпотребленіе:\n  {usage}\n\nПараметры и доводы:\n{all-args}{after-help}"
)]
struct Args {
    /// Страница книги на движкѣ diva.js (elib.rgo.ru/prlib.ru). Можно многократно.
    #[arg(long = "item")]
    items: Vec<String>,

    /// Прямой URL objectData-JSON (если не хочется парсить страницу книги).
    #[arg(long)]
    objectdata: Option<String>,

    /// Адресъ IIPImage-сервера (…/iipsrv.fcgi) — для --objectdata.
    #[arg(long)]
    iip: Option<String>,

    /// Путь imageDir на серверѣ скановъ — для --objectdata.
    #[arg(long)]
    imagedir: Option<String>,

    /// Произвольная HTML-страница, изъ коей брать ссылки на изображенія. Многократно.
    #[arg(long = "page")]
    pages: Vec<String>,

    /// Шаблонъ URL съ `{n}` для перебора номеровъ страницъ (см. --from/--to).
    #[arg(long)]
    url_template: Option<String>,

    /// Начальный номеръ для --url-template.
    #[arg(long, default_value_t = 1)]
    from: u32,

    /// Конечный номеръ (включительно) для --url-template.
    #[arg(long)]
    to: Option<u32>,

    /// Сколькими цифрами дополнять {n} нулями (0 — безъ дополненія).
    #[arg(long, default_value_t = 0)]
    pad: usize,

    /// Файлъ со списком ссылокъ на изображенія, по одной на строку.
    #[arg(long)]
    url_list: Option<PathBuf>,

    /// Коллекция RGO (DSpace handle), напр. https://elib.rgo.ru/handle/123456789/211882.
    /// Автоматически найдет всѣ книги в ней и поставит их в очередь.
    #[arg(long = "rgo-handle")]
    rgo_handles: Vec<String>,

    /// Выходная папка.
    #[arg(long, default_value = "rgo_jxl_dump")]
    out: PathBuf,

    /// Почтительная задержка между страницами (мсек).
    #[arg(long, default_value_t = 1500)]
    delay_ms: u64,

    /// Задержка между запросами плитокъ внутри одной страницы (мсек).
    #[arg(long, default_value_t = 150)]
    tile_delay_ms: u64,

    /// Размѣръ плитки при сборкѣ полнаго разрѣшенія (≤ ~1700, иначе серверъ уменьшитъ).
    #[arg(long, default_value_t = 1024)]
    tile: u32,

    /// Не собирать полное разрѣшеніе изъ плитокъ, а брать одиночный кадръ `/full/max/`
    /// (быстрѣе, но серверъ ограничиваетъ сторону ~1700 px).
    #[arg(long)]
    no_full_res: bool,

    /// Остановиться послѣ сего числа новыхъ сохраненій.
    #[arg(long)]
    limit: Option<usize>,

    /// Въ семъ запускѣ вновь пробовать неудавшіяся страницы, а не оставлять ихъ въ спискѣ ошибокъ.
    #[arg(long)]
    retry_failed: bool,

    /// HTTP-заголовокъ Referer (нѣкоторые серверы безъ него отдаютъ 403).
    #[arg(long, default_value = DEFAULT_REFERER)]
    referer: String,

    /// Butteraugli-разстояніе для JPEG XL съ потерями (0..15, меньше — лучше).
    /// 1.0 — зрительно безъ потерь. 0.0 — математически безъ потерь.
    #[arg(long, default_value_t = 1.0)]
    distance: f32,

    /// Честный lossless JPEG XL (полная обратимость по пикселямъ). Игнорируетъ --distance.
    #[arg(long)]
    lossless: bool,

    /// Скорость/усиліе кодировщика: lightning, thunder, falcon, cheetah, hare,
    /// wombat, squirrel, kitten, tortoise. Медленнѣе — меньше размѣръ.
    #[arg(long, default_value = "squirrel")]
    effort: String,

    /// Сохранять также собранный исходный JPEG въ raw/ (до пересжатія въ jxl).
    #[arg(long)]
    keep_raw: bool,

    /// Пересохранять, даже если .jxl уже существуетъ.
    #[arg(long)]
    redownload: bool,

    /// Писать state.json каждыхъ N обработанныхъ страницъ.
    #[arg(long, default_value_t = 25)]
    checkpoint_every: usize,

    /// Минимальный размѣръ загруженнаго файла въ байтахъ (защита отъ заглушекъ).
    #[arg(long, default_value_t = 1024)]
    min_bytes: usize,

    /// Не сохранять JXL (использовать вмѣстѣ съ --keep-raw).
    #[arg(long)]
    no_jxl: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct State {
    queue: VecDeque<Url>,
    seen: HashSet<Url>,
    downloaded: HashSet<Url>,
    failed: HashMap<Url, String>,
}

#[derive(Debug, Serialize)]
struct ManifestRecord {
    url: String,
    file_stem: String,
    jxl_path: String,
    raw_path: Option<String>,
    src_bytes: usize,
    jxl_bytes: usize,
    width: u32,
    height: u32,
    lossless: bool,
    distance: f32,
}

/// Настройки diva-плеера, извлечённыя изъ страницы книги.
#[derive(Debug, Clone)]
struct DivaPlayer {
    iip: String,
    image_dir: String,
    object_data: String,
}

#[derive(Debug, Deserialize)]
struct ObjectData {
    pgs: Vec<ObjPage>,
}

#[derive(Debug, Deserialize)]
struct ObjPage {
    /// имя файла скана (TIFF) на серверѣ
    f: String,
}

#[derive(Debug, Deserialize)]
struct IiifInfo {
    width: u32,
    height: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if !args.no_jxl {
        fs::create_dir_all(args.out.join("jxl"))?;
    }
    if args.keep_raw || args.no_jxl {
        fs::create_dir_all(args.out.join("raw"))?;
    }

    let client = build_client(&args.referer)?;
    let mut state = load_or_seed_state(&args, &client)?;

    if args.retry_failed {
        let failed_urls: Vec<Url> = state.failed.keys().cloned().collect();
        for url in failed_urls {
            if !state.downloaded.contains(&url) && !state.queue.contains(&url) {
                state.queue.push_back(url.clone());
            }
            state.failed.remove(&url);
        }
    }

    println!(
        "Checkpoint loaded: downloaded={}, queue={}, seen={}, failed={}, out={}",
        state.downloaded.len(),
        state.queue.len(),
        state.seen.len(),
        state.failed.len(),
        args.out.display()
    );

    let speed = parse_effort(&args.effort)?;
    let mut new_saves = 0usize;
    let mut processed = 0usize;

    while let Some(url) = state.queue.pop_front() {
        if !args.redownload && (state.downloaded.contains(&url) || jxl_already_saved(&args.out, &url))
        {
            state.downloaded.insert(url.clone());
            processed += 1;
            maybe_checkpoint(&args, &state, processed)?;
            continue;
        }

        if let Some(limit) = args.limit {
            if new_saves >= limit {
                state.queue.push_front(url);
                println!("Reached --limit {limit}; stopping.");
                break;
            }
        }

        match process_one(&args, &client, &url, speed) {
            Ok(rec) => {
                let ratio = if rec.src_bytes > 0 {
                    100.0 * rec.jxl_bytes as f64 / rec.src_bytes as f64
                } else {
                    0.0
                };
                println!(
                    "  saved {}  {}x{}  {}B -> {}B ({:.0}%)",
                    rec.file_stem, rec.width, rec.height, rec.src_bytes, rec.jxl_bytes, ratio
                );
                append_manifest(args.out.join("manifest.jsonl"), &rec)?;
                state.downloaded.insert(url.clone());
                state.failed.remove(&url);
                new_saves += 1;
            }
            Err(e) => {
                eprintln!("  FAIL {url}: {e:#}");
                state.failed.insert(url.clone(), format!("{e:#}"));
            }
        }

        processed += 1;
        maybe_checkpoint(&args, &state, processed)?;
        sleep(Duration::from_millis(args.delay_ms));
    }

    save_state(&args.out, &state)?;
    println!(
        "Done. downloaded={}, failed={}, remaining_queue={}",
        state.downloaded.len(),
        state.failed.len(),
        state.queue.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Засѣвъ очереди
// ---------------------------------------------------------------------------

fn load_or_seed_state(args: &Args, client: &Client) -> Result<State> {
    let state_path = args.out.join("state.json");
    let mut state: State = if state_path.exists() {
        let data = fs::read(&state_path).context("reading state.json")?;
        serde_json::from_slice(&data).context("parsing state.json")?
    } else {
        State::default()
    };

    let mut add = |state: &mut State, url: Url| {
        if !state.seen.contains(&url) && !state.downloaded.contains(&url) {
            state.seen.insert(url.clone());
            state.queue.push_back(url);
        }
    };

    // 1. Страницы книгъ diva.js -> настройки -> objectData -> списокъ страницъ.
    for item in &args.items {
        let item_url = Url::parse(item).with_context(|| format!("bad --item URL: {item}"))?;
        seed_from_item_or_viewer(client, &item_url, &mut state, &mut add)?;
    }

    // 2. Прямой objectData (+ iip/imagedir).
    if let Some(od) = &args.objectdata {
        let iip = args
            .iip
            .clone()
            .ok_or_else(|| anyhow!("--objectdata requires --iip"))?;
        let image_dir = args
            .imagedir
            .clone()
            .ok_or_else(|| anyhow!("--objectdata requires --imagedir"))?;
        let pl = DivaPlayer {
            iip,
            image_dir,
            object_data: od.clone(),
        };
        seed_from_player(client, &pl, &mut state, &mut add)?;
    }

    // 3. HTML-страницы -> ссылки на изображенія.
    for page in &args.pages {
        let page_url = Url::parse(page).with_context(|| format!("bad --page URL: {page}"))?;
        let html = fetch_text(client, &page_url)
            .with_context(|| format!("fetching page {page_url}"))?;
        let imgs = discover_image_links(&html, &page_url)?;
        println!("  page {page_url}: found {} image link(s)", imgs.len());
        for img in imgs {
            add(&mut state, img);
        }
    }

    // 4. Шаблонъ съ номерами страницъ.
    if let Some(tmpl) = &args.url_template {
        let to = args
            .to
            .ok_or_else(|| anyhow!("--url-template requires --to <N>"))?;
        for n in args.from..=to {
            let num = if args.pad > 0 {
                format!("{n:0width$}", width = args.pad)
            } else {
                n.to_string()
            };
            let s = tmpl.replace("{n}", &num);
            let url = Url::parse(&s).with_context(|| format!("bad templated URL: {s}"))?;
            add(&mut state, url);
        }
    }

    // 5. Списокъ ссылокъ изъ файла.
    if let Some(list) = &args.url_list {
        let body = fs::read_to_string(list)
            .with_context(|| format!("reading --url-list {}", list.display()))?;
        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let url = Url::parse(line).with_context(|| format!("bad URL in list: {line}"))?;
            
            // Если въ спискѣ ссылка на RGO или PRLIB, обрабатываемъ её какъ страницу книги/коллекціи
            let host = url.host_str().unwrap_or("");
            let path = url.path();
            
            let is_library_page = (host == "elib.rgo.ru" && (path.contains("/handle/") || path.contains("/safe-view/"))) ||
                                 (host.contains("prlib.ru") && path.contains("/item/"));

            if is_library_page {
                seed_from_item_or_viewer(client, &url, &mut state, &mut add)?;
                continue;
            }
            
            add(&mut state, url);
        }
    }

    // 6. Коллекции RGO (DSpace handles).
    for handle in &args.rgo_handles {
        let handle_url = Url::parse(handle).with_context(|| format!("bad --rgo-handle URL: {handle}"))?;
        seed_from_rgo_handle(client, &handle_url, &mut state, &mut add)
            .with_context(|| format!("seeding from RGO handle {handle_url}"))?;
    }

    if state.queue.is_empty() && state.downloaded.is_empty() {
        return Err(anyhow!(
            "no pages to fetch — give --item, --objectdata, --page, --url-template/--to, or --url-list"
        ));
    }
    Ok(state)
}

fn seed_from_item_or_viewer(
    client: &Client,
    url: &Url,
    state: &mut State,
    add: &mut dyn FnMut(&mut State, Url),
) -> Result<()> {
    let html = fetch_text(client, url)
        .with_context(|| format!("fetching {url}"))?;
    
    // 1. Попробуем Diva.js (IIIF)
    let players = extract_diva_players(&html);
    if !players.is_empty() {
        for pl in players {
            seed_from_player(client, &pl, state, add)?;
        }
        return Ok(());
    }

    // 2. Попробуем SafeView (custom RGO)
    if url.path().contains("/safe-view/") {
        if let Err(e) = seed_from_safe_view(&html, url, state, add) {
            eprintln!("  WARN: failed to seed from safe-view {}: {}", url, e);
        }
        return Ok(());
    }

    // 3. Попробуем RGO Collection (DSpace)
    if url.host_str() == Some("elib.rgo.ru") && url.path().contains("/handle/") {
        // Проверяем, коллекция это или предмет
        if html.contains("tbl-type-1-adapt") || html.contains("pagination") {
             return seed_from_rgo_handle(client, url, state, add);
        }
        
        // Если предмет, ищем кнопку Просмотр
        if let Some(viewer_url) = extract_viewer_from_item(client, url)? {
            return seed_from_item_or_viewer(client, &viewer_url, state, add);
        }
    }

    eprintln!("  WARN: no diva/safe-view players found on {url}");
    Ok(())
}

/// Загрузить objectData плеера и засѣять очередь IIIF-задачами (по одной на страницу).
fn seed_from_player(
    client: &Client,
    pl: &DivaPlayer,
    state: &mut State,
    add: &mut dyn FnMut(&mut State, Url),
) -> Result<()> {
    let od_url = Url::parse(&pl.object_data)
        .with_context(|| format!("bad objectData URL: {}", pl.object_data))?;
    let body = fetch_text(client, &od_url)?;
    let data: ObjectData = serde_json::from_str(&body)
        .with_context(|| format!("parsing objectData JSON from {od_url}"))?;
    println!(
        "  objectData {}: {} page(s)",
        pl.object_data,
        data.pgs.len()
    );
    let dir = pl.image_dir.trim_end_matches('/');
    for pg in &data.pgs {
        // info.json-URL служитъ и якоремъ задачи, и источникомъ размѣровъ страницы.
        let s = format!("{}?{IIIF_MARK}{dir}/{}{INFO_SUFFIX}", pl.iip, pg.f);
        match Url::parse(&s) {
            Ok(u) => add(state, u),
            Err(e) => eprintln!("  WARN: bad IIIF URL {s}: {e}"),
        }
    }
    Ok(())
}

/// Сѣять изъ коллекціи RGO (DSpace). Рекурсивно (по пагинаціи) находитъ всѣ книги.
fn seed_from_rgo_handle(
    client: &Client,
    root_url: &Url,
    state: &mut State,
    add: &mut dyn FnMut(&mut State, Url),
) -> Result<()> {
    let mut pages_to_visit = VecDeque::new();
    pages_to_visit.push_back(root_url.clone());

    let mut visited_pages = HashSet::new();
    let mut found_items = HashSet::new();

    while let Some(current_url) = pages_to_visit.pop_front() {
        if !visited_pages.insert(current_url.clone()) {
            continue;
        }

        println!("  crawling collection page {}...", current_url);
        let html = fetch_text(client, &current_url)?;
        let doc = Html::parse_document(&html);

        // 1. Находим ссылки на предметы (книги). Обычно /handle/123456789/NNN
        let sel_a = Selector::parse("a[href]").unwrap();
        for a in doc.select(&sel_a) {
            let Some(href) = a.value().attr("href") else { continue };
            if href.contains("#") { continue; } // Игнорируем якори

            let Ok(u) = root_url.join(href) else { continue };

            // Игнорируем саму себя, пагинацию и фильтры в качестве предметов
            if u.path() == root_url.path() || u.query().is_some() {
                if is_rgo_pagination_url(&u, root_url) && !visited_pages.contains(&u) {
                    pages_to_visit.push_back(u);
                }
                continue;
            }

            if is_rgo_item_url(&u) {
                if found_items.insert(u.clone()) {
                    println!("    found item: {}", u);
                    if let Err(e) = seed_from_item_or_viewer(client, &u, state, add) {
                        eprintln!("      WARN: failed to process item {}: {}", u, e);
                    }
                }
            }
        }
    }

    Ok(())
}

fn is_rgo_item_url(url: &Url) -> bool {
    let path = url.path();
    // Типичный путь предмета: /handle/123456789/227798
    // Избегаем путей коллекций (обычно заканчиваются на цифры, но в них нет вложенности дальше)
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segments.len() >= 3 && segments[0] == "handle" && segments[2].chars().all(|c| c.is_numeric())
}

fn is_rgo_pagination_url(url: &Url, root: &Url) -> bool {
    url.path() == root.path() && url.query().map(|q| q.contains("offset=")).unwrap_or(false)
}

fn extract_viewer_from_item(client: &Client, item_url: &Url) -> Result<Option<Url>> {
    let html = fetch_text(client, item_url)?;
    let doc = Html::parse_document(&html);
    let sel = Selector::parse("a.button-primary").unwrap();
    for a in doc.select(&sel) {
        if let Some(href) = a.value().attr("href") {
            if href.contains("/safe-view/") {
                return Ok(Some(item_url.join(href)?));
            }
        }
    }
    Ok(None)
}

/// Сѣять изъ SafeView (custom RGO viewer). Находитъ всѣ страницы по шаблону.
fn seed_from_safe_view(
    html: &str,
    viewer_url: &Url,
    state: &mut State,
    add: &mut dyn FnMut(&mut State, Url),
) -> Result<()> {
    // 1. Извлекаем количество страниц
    let re_count = Regex::new(r"setPagesCount\((\d+)\)").unwrap();
    let count = re_count.captures(html)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .ok_or_else(|| anyhow!("could not find pages count in safe-view HTML"))?;
    
    // 2. Извлекаем базовое имя файла из URL
    let url_str = viewer_url.as_str();
    let url_parts: Vec<&str> = url_str.split('/').collect();
    let filename_orig = url_parts.last().ok_or_else(|| anyhow!("empty safe-view URL"))?;
    
    // Убираем фрагмент если есть
    let filename_orig = filename_orig.split('#').next().unwrap();
    
    // Логика SafeView: b64decode(filename.replace('_', '/')) -> base + "/" + index -> b64encode.replace('/', '_')
    let filename_slash = filename_orig.replace('_', "/");
    let decoded_bytes = BASE64.decode(filename_slash)
        .map_err(|e| anyhow!("failed to decode safe-view filename {}: {}", filename_orig, e))?;
    let decoded_str = String::from_utf8(decoded_bytes)
        .map_err(|e| anyhow!("safe-view filename is not UTF-8: {}", e))?;
    
    let base_viewer_url = &url_str[..url_str.len() - filename_orig.len()];

    println!("      safe-view: found {} pages", count);
    for i in 0..count {
        let target = format!("{}/{}", decoded_str, i);
        let encoded = BASE64.encode(target).replace('/', "_");
        let img_url_str = format!("{}{}", base_viewer_url, encoded);
        if let Ok(u) = Url::parse(&img_url_str) {
            add(state, u);
        }
    }

    Ok(())
}

/// Извлечь настройки diva.js изъ HTML страницы (Drupal.settings).
fn extract_diva_players(html: &str) -> Vec<DivaPlayer> {
    // Порядокъ полей въ options постояненъ: iipServerURL, imageDir, objectData.
    let re = Regex::new(
        r#""iipServerURL"\s*:\s*"(.*?)"\s*,\s*"imageDir"\s*:\s*"(.*?)"\s*,\s*"objectData"\s*:\s*"(.*?)""#,
    )
    .expect("valid regex");
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for c in re.captures_iter(html) {
        let pl = DivaPlayer {
            iip: json_unescape(&c[1]),
            image_dir: json_unescape(&c[2]),
            object_data: json_unescape(&c[3]),
        };
        if seen.insert(pl.object_data.clone()) {
            out.push(pl);
        }
    }
    out
}

/// Развернуть `\/` и `\uXXXX` въ строкахъ изъ встроеннаго JSON.
fn json_unescape(s: &str) -> String {
    // Простой и достаточный разворотъ для URL/путей: оборачиваемъ въ JSON-строку
    // и просимъ serde разобрать; при неудачѣ — хотя бы `\/` -> `/`.
    match serde_json::from_str::<String>(&format!("\"{s}\"")) {
        Ok(v) => v,
        Err(_) => s.replace("\\/", "/"),
    }
}

// ---------------------------------------------------------------------------
// Обработка одной страницы
// ---------------------------------------------------------------------------

fn process_one(
    args: &Args,
    client: &Client,
    url: &Url,
    speed: EncoderSpeed,
) -> Result<ManifestRecord> {
    let url_str = url.as_str();
    let is_iiif = url_str.contains(IIIF_MARK) && url_str.ends_with(INFO_SUFFIX);
    let (jpeg_bytes, width, height, stem, can_transcode) = if is_iiif {
        let (b, w, h, s) = fetch_iiif_page(args, client, url)?;
        (b, w, h, s, false)
    } else {
        let (bytes, content_type) = fetch_bytes(client, url).with_context(|| format!("downloading {url}"))?;
        if bytes.len() < args.min_bytes {
            return Err(anyhow!(
                "response too small ({} bytes < --min-bytes {})",
                bytes.len(),
                args.min_bytes
            ));
        }
        
        let img = match decode_image(&bytes) {
            Ok(img) => img,
            Err(e) => {
                let ct = content_type.unwrap_or_else(|| "unknown".to_string());
                let sample = String::from_utf8_lossy(&bytes[..bytes.len().min(100)]).replace('\n', " ");
                return Err(anyhow!(
                    "decoding image failed. Content-Type: {}, Error: {}. First 100 bytes: [{}]",
                    ct, e, sample
                ));
            }
        };
        
        let (w, h) = (img.width(), img.height());
        (bytes, w, h, file_stem_for_url(url), true)
    };

    let mut jxl_bytes = 0;
    let mut jxl_rel = String::new();

    if !args.no_jxl {
        // Декодируемъ только если нужно кодировать изъ пикселей
        let jxl = if can_transcode && (args.lossless || args.distance <= 0.1) {
            // Прямое транскодированіе (lossless DCT)
            let mut encoder = encoder_builder()
                .speed(speed)
                .build()
                .map_err(|e| anyhow!("building jxl encoder: {e}"))?;
            
            match encoder.encode_jpeg(&jpeg_bytes) {
                Ok(data) => data.data,
                Err(e) => {
                    eprintln!("  WARN: transcode failed, falling back to pixels: {}", e);
                    let rgb = decode_image(&jpeg_bytes)?;
                    encode_jxl(&rgb, rgb.width(), rgb.height(), args.lossless, args.distance, speed)?
                }
            }
        } else {
            let rgb = decode_image(&jpeg_bytes)?;
            encode_jxl(&rgb, rgb.width(), rgb.height(), args.lossless, args.distance, speed)
                .context("encoding JPEG XL")?
        };

        jxl_bytes = jxl.len();
        jxl_rel = format!("jxl/{stem}.jxl");
        atomic_write(args.out.join(&jxl_rel), &jxl)?;
    }

    let raw_rel = if args.keep_raw || args.no_jxl {
        let rel = format!("raw/{stem}.jpg");
        atomic_write(args.out.join(&rel), &jpeg_bytes)?;
        Some(rel)
    } else {
        None
    };

    Ok(ManifestRecord {
        url: url_str.to_string(),
        file_stem: stem,
        jxl_path: jxl_rel,
        raw_path: raw_rel,
        src_bytes: jpeg_bytes.len(),
        jxl_bytes,
        width,
        height,
        lossless: args.lossless,
        distance: if args.lossless { 0.0 } else { args.distance },
    })
}

/// Собрать страницу черезъ IIIF: info.json -> полное разрѣшеніе изъ плитокъ -> JPEG.
/// Возвращаетъ (jpeg_bytes, width, height, file_stem).
fn fetch_iiif_page(
    args: &Args,
    client: &Client,
    info_url: &Url,
) -> Result<(Vec<u8>, u32, u32, String)> {
    let info_str = info_url.as_str().to_string();
    let base = info_str
        .strip_suffix(INFO_SUFFIX)
        .ok_or_else(|| anyhow!("not an info.json URL: {info_str}"))?
        .to_string();
    let stem = iiif_stem(&base);

    let info_body = fetch_text(client, info_url)
        .with_context(|| format!("fetching {info_url}"))?;
    let info: IiifInfo = serde_json::from_str(&info_body)
        .with_context(|| format!("parsing IIIF info.json from {info_url}"))?;

    let (w, h) = (info.width, info.height);

    if args.no_full_res {
        // Одинъ кадръ съ ограниченіемъ сервера по сторонѣ.
        let one = format!("{base}/full/max/0/default.jpg");
        let u = Url::parse(&one)?;
        let (bytes, _) = fetch_bytes(client, &u)?;
        let img = decode_image(&bytes)?;
        return Ok((bytes, img.width(), img.height(), stem));
    }

    // Полное разрѣшеніе — сборка изъ плитокъ.
    let tile = args.tile.max(64);
    let mut canvas = RgbImage::new(w, h);
    let mut first = true;
    let mut y = 0u32;
    while y < h {
        let rh = tile.min(h - y);
        let mut x = 0u32;
        while x < w {
            let rw = tile.min(w - x);
            let region = format!("{base}/{x},{y},{rw},{rh}/full/0/default.jpg");
            let u = Url::parse(&region)?;
            if !first && args.tile_delay_ms > 0 {
                sleep(Duration::from_millis(args.tile_delay_ms));
            }
            first = false;
            let (bytes, _) = fetch_bytes(client, &u)
                .with_context(|| format!("fetching tile {region}"))?;
            let tile_img = decode_image(&bytes)
                .with_context(|| format!("decoding tile {region}"))?;
            blit(&mut canvas, &tile_img, x, y);
            x += rw;
        }
        y += rh;
    }

    let jpeg = encode_jpeg(&canvas).context("encoding stitched JPEG")?;
    Ok((jpeg, w, h, stem))
}

/// Скопировать плитку въ холстъ въ позицію (ox, oy), не выходя за края.
fn blit(canvas: &mut RgbImage, tile: &RgbImage, ox: u32, oy: u32) {
    let (cw, ch) = (canvas.width(), canvas.height());
    for ty in 0..tile.height() {
        let dy = oy + ty;
        if dy >= ch {
            break;
        }
        for tx in 0..tile.width() {
            let dx = ox + tx;
            if dx >= cw {
                break;
            }
            canvas.put_pixel(dx, dy, *tile.get_pixel(tx, ty));
        }
    }
}

fn iiif_stem(base: &str) -> String {
    // base оканчивается именемъ файла: …/<file>.tiff
    let last = base.rsplit('/').next().unwrap_or(base);
    let name = last.split('?').next().unwrap_or(last);
    let stem = sanitize_stem(name);
    if stem.is_empty() {
        "page".to_string()
    } else {
        stem
    }
}

// ---------------------------------------------------------------------------
// Изображенія / кодеки
// ---------------------------------------------------------------------------

fn decode_image(bytes: &[u8]) -> Result<RgbImage> {
    let dynimg = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("guessing image format")?
        .decode()
        .context("decoding image")?;
    Ok(dynimg.to_rgb8())
}

fn encode_jpeg(img: &RgbImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 95);
    enc.encode_image(img).context("jpeg encode")?;
    Ok(buf)
}

fn encode_jxl(
    rgb: &RgbImage,
    width: u32,
    height: u32,
    lossless: bool,
    distance: f32,
    speed: EncoderSpeed,
) -> Result<Vec<u8>> {
    let mut encoder = encoder_builder()
        .has_alpha(false)
        .lossless(lossless)
        .speed(speed)
        .build()
        .map_err(|e| anyhow!("building jxl encoder: {e}"))?;

    if lossless {
        // libjxl требуетъ оригинальный профиль для честного lossless.
        encoder.uses_original_profile = true;
    } else {
        encoder.quality = distance; // butteraugli distance, lower = better
    }

    let result: jpegxl_rs::encode::EncoderResult<u8> = encoder
        .encode(rgb.as_raw(), width, height)
        .map_err(|e| anyhow!("jxl encode failed: {e}"))?;
    Ok(result.data)
}

fn parse_effort(name: &str) -> Result<EncoderSpeed> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "lightning" => EncoderSpeed::Lightning,
        "thunder" => EncoderSpeed::Thunder,
        "falcon" => EncoderSpeed::Falcon,
        "cheetah" => EncoderSpeed::Cheetah,
        "hare" => EncoderSpeed::Hare,
        "wombat" => EncoderSpeed::Wombat,
        "squirrel" => EncoderSpeed::Squirrel,
        "kitten" => EncoderSpeed::Kitten,
        "tortoise" => EncoderSpeed::Tortoise,
        other => return Err(anyhow!("unknown --effort '{other}'")),
    })
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn build_client(referer: &str) -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (compatible; rgo-jxl-archiver/0.2; +offline personal archive)",
        ),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("image/avif,image/webp,image/*,*/*;q=0.8"),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("ru,en;q=0.8"));
    if let Ok(v) = HeaderValue::from_str(referer) {
        headers.insert(REFERER, v);
    }

    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .context("building HTTP client")
}

fn fetch_bytes(client: &Client, url: &Url) -> Result<(Vec<u8>, Option<String>)> {
    let resp = client.get(url.clone()).send()?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("HTTP {} for {url}", status.as_u16()));
    }
    let content_type = resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    Ok((resp.bytes()?.to_vec(), content_type))
}

fn fetch_text(client: &Client, url: &Url) -> Result<String> {
    let (bytes, _) = fetch_bytes(client, url)?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

// ---------------------------------------------------------------------------
// Извлеченіе ссылокъ на изображенія изъ HTML (для --page)
// ---------------------------------------------------------------------------

fn discover_image_links(html: &str, base: &Url) -> Result<Vec<Url>> {
    let doc = Html::parse_document(html);
    let mut out: Vec<Url> = Vec::new();
    let mut seen: HashSet<Url> = HashSet::new();

    let push = |raw: &str, out: &mut Vec<Url>, seen: &mut HashSet<Url>| {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with("data:") {
            return;
        }
        if let Ok(u) = base.join(raw) {
            if looks_like_image(&u) && seen.insert(u.clone()) {
                out.push(u);
            }
        }
    };

    if let Ok(sel) = Selector::parse("img") {
        for el in doc.select(&sel) {
            for attr in ["src", "data-src", "data-original", "data-lazy-src"] {
                if let Some(v) = el.value().attr(attr) {
                    push(v, &mut out, &mut seen);
                }
            }
            if let Some(srcset) = el.value().attr("srcset") {
                if let Some(best) = largest_from_srcset(srcset) {
                    push(&best, &mut out, &mut seen);
                }
            }
        }
    }

    if let Ok(sel) = Selector::parse("a[href]") {
        for el in doc.select(&sel) {
            if let Some(v) = el.value().attr("href") {
                push(v, &mut out, &mut seen);
            }
        }
    }

    if let Ok(sel) = Selector::parse("source") {
        for el in doc.select(&sel) {
            if let Some(v) = el.value().attr("src") {
                push(v, &mut out, &mut seen);
            }
            if let Some(srcset) = el.value().attr("srcset") {
                if let Some(best) = largest_from_srcset(srcset) {
                    push(&best, &mut out, &mut seen);
                }
            }
        }
    }

    if let Ok(sel) = Selector::parse(r#"meta[property="og:image"], meta[name="twitter:image"]"#) {
        for el in doc.select(&sel) {
            if let Some(v) = el.value().attr("content") {
                push(v, &mut out, &mut seen);
            }
        }
    }

    Ok(out)
}

fn largest_from_srcset(srcset: &str) -> Option<String> {
    let mut best: Option<(u32, String)> = None;
    for part in srcset.split(',') {
        let part = part.trim();
        let mut it = part.split_whitespace();
        let url = it.next()?.to_string();
        let w = it
            .next()
            .and_then(|d| d.strip_suffix('w'))
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(0);

        match &best {
            Some((bw, _)) if *bw >= w => {}
            _ => best = Some((w, url)),
        }
    }
    best.map(|(_, u)| u)
}

fn looks_like_image(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    const EXTS: [&str; 8] = [
        ".jpg", ".jpeg", ".png", ".tif", ".tiff", ".webp", ".bmp", ".jp2",
    ];
    if EXTS.iter().any(|e| path.ends_with(e)) {
        return true;
    }
    let q = url.query().unwrap_or("").to_ascii_lowercase();
    path.contains("view") || path.contains("image") || path.contains("page") || q.contains("image")
}

// ---------------------------------------------------------------------------
// Имена файловъ / атомарная запись / состояніе
// ---------------------------------------------------------------------------

fn file_stem_for_url(url: &Url) -> String {
    let mut s = String::new();
    if let Some(host) = url.host_str() {
        s.push_str(host);
        s.push('_');
    }
    s.push_str(url.path());
    if let Some(q) = url.query() {
        s.push('_');
        s.push_str(q);
    }
    let stem = sanitize_stem(&s);
    if stem.is_empty() {
        "index".to_string()
    } else {
        stem
    }
}

fn sanitize_stem(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_us = false;
    for c in input.chars() {
        let ok = c.is_ascii_alphanumeric() || c == '-' || c == '.';
        if ok {
            out.push(c);
            prev_us = false;
        } else if !prev_us {
            out.push('_');
            prev_us = true;
        }
    }
    let trimmed = out.trim_matches('_').trim_matches('.').to_string();
    if trimmed.len() > 180 {
        trimmed[trimmed.len() - 180..].to_string()
    } else {
        trimmed
    }
}

fn jxl_already_saved(out: &Path, url: &Url) -> bool {
    let stem = if url.as_str().contains(IIIF_MARK) && url.as_str().ends_with(INFO_SUFFIX) {
        iiif_stem(url.as_str().strip_suffix(INFO_SUFFIX).unwrap_or(url.as_str()))
    } else {
        file_stem_for_url(url)
    };
    out.join(format!("jxl/{stem}.jxl")).exists() || out.join(format!("raw/{stem}.jpg")).exists()
}

fn maybe_checkpoint(args: &Args, state: &State, processed: usize) -> Result<()> {
    let every = args.checkpoint_every.max(1);
    if processed % every == 0 {
        save_state(&args.out, state)?;
    }
    Ok(())
}

fn save_state(out: &Path, state: &State) -> Result<()> {
    let data = serde_json::to_vec(state).context("serializing state")?;
    atomic_write(out.join("state.json"), &data)
}

fn append_manifest(path: PathBuf, record: &ManifestRecord) -> Result<()> {
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
    fs::rename(&tmp, &path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srcset_picks_largest() {
        let s = "a.jpg 100w, b.jpg 800w, c.jpg 400w";
        assert_eq!(largest_from_srcset(s).as_deref(), Some("b.jpg"));
    }

    #[test]
    fn stem_is_sanitised() {
        let u = Url::parse("https://elib.rgo.ru/safe-view/123/0/5.jpg").unwrap();
        let stem = file_stem_for_url(&u);
        assert!(stem.contains("elib.rgo.ru"));
        assert!(stem.ends_with("5.jpg"));
        assert!(!stem.contains('/'));
    }

    #[test]
    fn image_detection() {
        assert!(looks_like_image(
            &Url::parse("https://x/y/page-1.JPEG").unwrap()
        ));
        assert!(looks_like_image(
            &Url::parse("https://elib.rgo.ru/safe-view/1/0/3").unwrap()
        ));
        assert!(!looks_like_image(&Url::parse("https://x/about.html").unwrap()));
    }

    #[test]
    fn diva_settings_are_parsed() {
        let html = r#"...{"diva":{"1":{"options":{"iipServerURL":"https:\/\/content.prlib.ru\/fcgi-bin\/iipsrv.fcgi","imageDir":"\/var\/data\/scans\/public\/ABC\/6633009","objectData":"https:\/\/content.prlib.ru\/metadata\/public\/ABC\/6633009\/ABC.json","zoomLevel":2}}}}..."#;
        let players = extract_diva_players(html);
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].iip, "https://content.prlib.ru/fcgi-bin/iipsrv.fcgi");
        assert_eq!(players[0].image_dir, "/var/data/scans/public/ABC/6633009");
        assert!(players[0].object_data.ends_with("ABC.json"));
    }

    #[test]
    fn iiif_stem_from_base() {
        let base = "https://content.prlib.ru/fcgi-bin/iipsrv.fcgi?IIIF=/var/data/scans/public/ABC/6633009/6633010_doc1_B09C.tiff";
        let stem = iiif_stem(base);
        assert!(stem.starts_with("6633010_doc1"));
        assert!(stem.ends_with(".tiff"));
    }

    #[test]
    fn blit_clips_to_canvas() {
        let mut canvas = RgbImage::new(4, 4);
        let mut tile = RgbImage::new(3, 3);
        for p in tile.pixels_mut() {
            *p = image::Rgb([10, 20, 30]);
        }
        // помѣщаемъ плитку у праваго нижняго угла — часть выходитъ за край
        blit(&mut canvas, &tile, 2, 2);
        assert_eq!(*canvas.get_pixel(2, 2), image::Rgb([10, 20, 30]));
        assert_eq!(*canvas.get_pixel(3, 3), image::Rgb([10, 20, 30]));
        // (0,0) осталось чёрнымъ
        assert_eq!(*canvas.get_pixel(0, 0), image::Rgb([0, 0, 0]));
    }
}
