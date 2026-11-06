use axum::{
    extract::{Path, Query},
    http::{
        header::{LOCATION, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::IntoResponse,
    routing::{get, patch, post},
    Extension, Json, Router,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_service::Service;
use wasm_bindgen::JsValue;
use worker::*;

mod auth;
mod borders;
mod continents;
mod email;
mod game;
mod guest;
mod oauth;
mod ratelimit;
mod util;

const GUEST_LIMIT_PER_IP: u32 = 3;
const FREEPLAY_GAME_SECS: i64 = 24 * 60 * 60;
const GUEST_IP_KEEP_DAYS: u32 = 2;
const GUEST_PLAYS_KEEP_DAYS: u32 = 14;
const CLEAR_SESSION: &str = "session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax";

enum AppError {
    BadRequest(&'static str),
    NotFound(&'static str),
    Conflict(&'static str),
    Unauthorized(&'static str),
    TooManyRequests(&'static str),
    Internal(String),
}

type ApiResult<T> = std::result::Result<Json<T>, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            AppError::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, msg),
            AppError::Internal(details) => {
                console_error!("internal error: {details}");
                (StatusCode::INTERNAL_SERVER_ERROR, "something went wrong")
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<worker::Error> for AppError {
    fn from(e: worker::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

enum Player {
    User(u32),
    Guest(String),
}

impl Player {
    fn key(&self) -> String {
        match self {
            Player::User(id) => format!("u:{id}"),
            Player::Guest(id) => format!("g:{id}"),
        }
    }
}

#[derive(Deserialize)]
struct NameRow {
    id: u32,
    name: String,
    first_city: u32,
    city_count: u32,
}

#[derive(Serialize, Deserialize)]
struct CityRow {
    name: String,
    country: String,
    lat: f64,
    lon: f64,
}

#[derive(Serialize)]
struct GuessResponse {
    distance_m: f64,
    score: u32,
    closest: usize,
    cities: Vec<CityRow>,
    city_limits: Option<&'static str>,
}

#[derive(Deserialize)]
struct CityAreaRow {
    city_id: u32,
    shape: Vec<u8>,
}

#[derive(Serialize)]
struct NextRound {
    round: u32,
    name: String,
}

#[derive(Serialize)]
struct PlayedRound {
    name: String,
    score: Option<u32>,
}

#[derive(Serialize)]
struct DailyState {
    date: String,
    number: u32,
    total: u32,
    history: Vec<PlayedRound>,
    next: Option<NextRound>,
}

#[derive(Deserialize)]
struct DailyStartRequest {
    date: Option<String>,
}

#[derive(Deserialize)]
struct ArchivePlayRow {
    date: String,
    round: u32,
    score: u32,
}

#[derive(Serialize)]
struct ArchiveDay {
    date: String,
    number: u32,
    round: u32,
    score: u32,
    cities: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ArchiveResponse {
    today: String,
    days: Vec<ArchiveDay>,
}

#[derive(Deserialize)]
struct DailyNamesRow {
    date: String,
    name_ids: String,
}

#[derive(Deserialize)]
struct IdNameRow {
    id: u32,
    name: String,
}

#[derive(Deserialize)]
struct PlayRow {
    round: u32,
    score: u32,
}

#[derive(Deserialize)]
struct StreakRow {
    date: String,
    finished_on: Option<String>,
}

#[derive(Deserialize)]
struct GuessPlayRow {
    round: u32,
    replays: u32,
}

#[derive(Deserialize)]
struct PlayHistoryRow {
    round: u32,
    score: u32,
    scores: String,
}

#[derive(Deserialize)]
struct DailyGuessRequest {
    date: String,
    lat: f64,
    lon: f64,
}

#[derive(Serialize)]
struct RoundGuessResponse {
    result: GuessResponse,
    total: u32,
    next: Option<NextRound>,
    previous_best: Option<u32>,
}

#[derive(Deserialize)]
struct FreeplayRow {
    round: u32,
    name_id: Option<u32>,
    pool: String,
    level: Option<u32>,
    seen: String,
}

#[derive(Deserialize, Default)]
struct FreeplayStartRequest {
    region: Option<String>,
    level: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct RegionBest {
    region: String,
    best: u32,
}

#[derive(Serialize)]
struct FreeplayStart {
    game_id: String,
    next: NextRound,
}

#[derive(Deserialize)]
struct FreeplayGuessRequest {
    game_id: String,
    lat: f64,
    lon: f64,
}

#[derive(Deserialize)]
struct CountRow {
    n: u32,
}

#[derive(Deserialize)]
struct BucketRow {
    bucket: u32,
    count: u32,
}

#[derive(Serialize)]
struct HistogramResponse {
    date: String,
    your_bucket: Option<u32>,
    bucket_starts: Vec<u32>,
    counts: Vec<u32>,
    players: u32,
    your_score: Option<u32>,
    better_than_percent: Option<u32>,
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct IdRow {
    id: u32,
}

#[derive(Deserialize)]
struct LoginRow {
    id: u32,
    pw_hash: String,
}

#[derive(Serialize)]
struct Profile {
    email: String,
    email_verified: bool,
    has_password: bool,
    is_admin: bool,
    providers: Vec<String>,
    daily_best: u32,
    streak: u32,
    region_bests: Vec<RegionBest>,
    best_score: u32,
    theme: String,
    units: String,
}

#[derive(Deserialize)]
struct ProfileRow {
    email: String,
    verified: u32,
    has_password: u32,
    is_admin: u32,
    providers: Option<String>,
    best_score: u32,
    theme: String,
    units: String,
    daily_best: u32,
}

#[derive(Deserialize)]
struct OAuthStartQuery {
    #[serde(rename = "return")]
    return_to: Option<String>,
}

#[derive(Deserialize)]
struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct UserIdRow {
    user_id: u32,
}

#[derive(Deserialize)]
struct ExistingUserRow {
    id: u32,
    verified: u32,
}

#[derive(Deserialize)]
struct TokenRequest {
    token: String,
}

#[derive(Deserialize)]
struct EmailRequest {
    email: String,
}

#[derive(Deserialize)]
struct ResetRequest {
    token: String,
    password: String,
}

#[derive(Deserialize)]
struct DeleteRequest {
    password: Option<String>,
}

#[derive(Deserialize)]
struct EmailRow {
    email: String,
}

#[derive(Deserialize)]
struct SettingsRequest {
    theme: Option<String>,
    units: Option<String>,
}

fn router(env: Env) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/freeplay", post(freeplay_start))
        .route("/api/freeplay/guess", post(freeplay_guess))
        .route("/api/daily", post(daily_start))
        .route("/api/daily/guess", post(daily_guess))
        .route("/api/daily/histogram/{date}", get(daily_histogram))
        .route("/api/daily/archive", get(daily_archive))
        .route("/api/daily/replay", post(daily_replay))
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/auth/google", get(google_start))
        .route("/api/auth/google/callback", get(google_callback))
        .route("/api/me", get(me).delete(delete_account))
        .route("/api/me/settings", patch(update_settings))
        .route("/api/me/verification-email", post(resend_verification))
        .route("/api/verify-email", post(verify_email))
        .route("/api/forgot-password", post(forgot_password))
        .route("/api/reset-password", post(reset_password))
        .layer(Extension(env))
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<axum::http::Response<axum::body::Body>> {
    Ok(router(env).call(req).await?)
}

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    if let Err(e) = cleanup(&env).await {
        console_error!("cleanup failed: {e}");
    }
}

async fn cleanup(env: &Env) -> worker::Result<()> {
    let db = env.d1("DB")?;
    let now = util::now_secs();
    let ip_cutoff = utc_date(GUEST_IP_KEEP_DAYS);
    let plays_cutoff = utc_date(GUEST_PLAYS_KEEP_DAYS);

    let results = db
        .batch(vec![
            db.prepare("DELETE FROM sessions WHERE expires_at < ?1")
                .bind(&[(now as f64).into()])?,
            db.prepare("DELETE FROM email_tokens WHERE expires_at < ?1")
                .bind(&[(now as f64).into()])?,
            db.prepare("DELETE FROM guest_ip_counts WHERE date < ?1")
                .bind(&[ip_cutoff.as_str().into()])?,
            db.prepare("DELETE FROM daily_plays WHERE player_id LIKE 'g:%' AND date < ?1")
                .bind(&[plays_cutoff.as_str().into()])?,
            db.prepare("DELETE FROM freeplay_games WHERE created_at < ?1")
                .bind(&[((now - FREEPLAY_GAME_SECS) as f64).into()])?,
            db.prepare("DELETE FROM auth_attempts WHERE window_start < ?1")
                .bind(&[((now - ratelimit::KEEP_SECS) as f64).into()])?,
        ])
        .await?;

    let deleted: Vec<String> = results
        .iter()
        .map(|r| {
            r.meta()
                .ok()
                .flatten()
                .and_then(|m| m.changes)
                .map_or("?".to_string(), |n| n.to_string())
        })
        .collect();
    console_log!(
        "cleanup deleted sessions/email_tokens/guest_ip_counts/guest daily_plays/freeplay_games/auth_attempts: {}",
        deleted.join("/")
    );
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn find_name(db: &D1Database, id: u32) -> worker::Result<Option<NameRow>> {
    db.prepare("SELECT id, name, first_city, city_count FROM names WHERE id = ?1")
        .bind(&[id.into()])?
        .first(None)
        .await
}

async fn pick_round_name(
    db: &D1Database,
    pool: &str,
    round: u32,
    draw: f64,
) -> worker::Result<Option<u32>> {
    let row: Option<IdRow> = db
        .prepare(
            "SELECT name_id AS id FROM round_names WHERE pool = ?1 AND round = ?2 AND cum > ?3 \
             ORDER BY cum LIMIT 1",
        )
        .bind(&[pool.into(), round.into(), draw.into()])?
        .first(None)
        .await?;
    Ok(row.map(|r| r.id))
}

async fn pick_unseen_name(
    db: &D1Database,
    pool: &str,
    round: u32,
    draw: f64,
    seen: &[u32],
) -> worker::Result<Option<u32>> {
    let placeholders: String = (0..seen.len()).map(|i| format!(", ?{}", i + 4)).collect();
    let mut values: Vec<JsValue> = vec![pool.into(), round.into(), draw.into()];
    values.extend(seen.iter().map(|&id| JsValue::from(id)));
    let row: Option<IdRow> = db
        .prepare(format!(
            "SELECT name_id AS id FROM round_names WHERE pool = ?1 AND round = ?2 AND cum > ?3 \
             AND name_id NOT IN (0{placeholders}) ORDER BY cum LIMIT 1"
        ))
        .bind(&values)?
        .first(None)
        .await?;
    Ok(row.map(|r| r.id))
}

async fn random_name_for_round(
    db: &D1Database,
    pool: &str,
    round: u32,
    seen: &[u32],
) -> std::result::Result<NameRow, AppError> {
    let draw = js_sys::Math::random();
    let mut id = pick_unseen_name(db, pool, round, draw, seen).await?;
    if id.is_none() {
        id = pick_unseen_name(db, pool, round, -1.0, seen).await?;
    }
    if id.is_none() {
        id = pick_round_name(db, pool, round, draw).await?;
    }
    let id = match id {
        Some(id) => id,
        None => {
            let (min, max) = game::id_range_for_round(round)
                .ok_or_else(|| AppError::Internal(format!("no id range for round {round}")))?;
            random_id(min, max)
        }
    };
    find_name(db, id)
        .await?
        .ok_or_else(|| AppError::Internal(format!("random name id {id} not found")))
}

async fn daily_ids(
    db: &D1Database,
    secret: &str,
    date: &str,
) -> std::result::Result<[u32; 5], AppError> {
    let saved: Option<DailyNamesRow> = db
        .prepare("SELECT date, name_ids FROM daily_names WHERE date = ?1")
        .bind(&[date.into()])?
        .first(None)
        .await?;
    if let Some(ids) = saved.and_then(|row| game::parse_name_ids(&row.name_ids)) {
        return Ok(ids);
    }

    let seed = game::daily_seed(secret, date);
    let ids = if date < game::FAME_START {
        game::daily_name_ids(seed)
    } else {
        let mut ids = [0u32; 5];
        for (i, draw) in game::daily_draws(seed).into_iter().enumerate() {
            let round = i as u32 + 1;
            ids[i] = pick_round_name(db, game::WORLD, round, draw)
                .await?
                .ok_or_else(|| {
                    AppError::Internal(format!("round_names has nothing for round {round}"))
                })?;
        }
        ids
    };

    db.prepare("INSERT INTO daily_names (date, name_ids) VALUES (?1, ?2) ON CONFLICT DO NOTHING")
        .bind(&[date.into(), game::format_name_ids(&ids).into()])?
        .run()
        .await?;
    Ok(ids)
}

#[worker::send]
async fn freeplay_start(
    Extension(env): Extension<Env>,
    body: Option<Json<FreeplayStartRequest>>,
) -> ApiResult<FreeplayStart> {
    let options = body.map(|Json(req)| req).unwrap_or_default();
    let pool = options.region.unwrap_or_else(|| game::WORLD.to_string());
    if !game::REGIONS.contains(&pool.as_str()) {
        return Err(AppError::BadRequest("unknown region"));
    }
    if options.level.is_some_and(|l| !game::LEVELS.contains(&l)) {
        return Err(AppError::BadRequest("level must be 1 to 5"));
    }

    let db = env.d1("DB")?;
    let now = util::now_secs();

    db.prepare("DELETE FROM freeplay_games WHERE created_at < ?1")
        .bind(&[((now - FREEPLAY_GAME_SECS) as f64).into()])?
        .run()
        .await?;

    let name = random_name_for_round(&db, &pool, options.level.unwrap_or(1), &[]).await?;
    let game_id = util::random_uuid()?;
    let level = options.level.map_or(JsValue::NULL, JsValue::from);
    db.prepare(
        "INSERT INTO freeplay_games (id, name_id, created_at, pool, level, seen) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&[
        game_id.as_str().into(),
        name.id.into(),
        (now as f64).into(),
        pool.as_str().into(),
        level,
        name.id.to_string().into(),
    ])?
    .run()
    .await?;

    Ok(Json(FreeplayStart {
        game_id,
        next: NextRound {
            round: 1,
            name: name.name,
        },
    }))
}

#[worker::send]
async fn freeplay_guess(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Json(req): Json<FreeplayGuessRequest>,
) -> ApiResult<RoundGuessResponse> {
    validate_coords(req.lat, req.lon)?;
    let db = env.d1("DB")?;

    let row: FreeplayRow = db
        .prepare("SELECT round, name_id, pool, level, seen FROM freeplay_games WHERE id = ?1")
        .bind(&[req.game_id.as_str().into()])?
        .first(None)
        .await?
        .ok_or(AppError::NotFound("game not found, start a new one"))?;
    let Some(name_id) = row.name_id else {
        return Err(AppError::BadRequest("already finished"));
    };

    let result = score_guess(&db, name_id, req.lat, req.lon, true, &row.pool).await?;

    let seen: Vec<u32> = row.seen.split(',').filter_map(|s| s.parse().ok()).collect();
    let next_name = if row.round + 1 < 5 {
        let band = row.level.unwrap_or(row.round + 2);
        Some(random_name_for_round(&db, &row.pool, band, &seen).await?)
    } else {
        None
    };
    let next_id = next_name.as_ref().map_or(JsValue::NULL, |n| n.id.into());
    let seen_after = match &next_name {
        Some(n) => format!("{},{}", row.seen, n.id),
        None => row.seen.clone(),
    };

    let updated: PlayRow = db
        .prepare(
            "UPDATE freeplay_games SET round = round + 1, score = score + ?1, name_id = ?2, seen = ?5 \
             WHERE id = ?3 AND round = ?4 \
             RETURNING round, score",
        )
        .bind(&[
            result.score.into(),
            next_id,
            req.game_id.as_str().into(),
            row.round.into(),
            seen_after.as_str().into(),
        ])?
        .first(None)
        .await?
        .ok_or(AppError::Conflict("round already answered"))?;

    let mut previous_best = None;
    if next_name.is_none() && row.level.is_none() {
        if let Some(user_id) = auth::current_user_id(&db, &headers).await? {
            previous_best = if row.pool == game::WORLD {
                update_best(&db, user_id, updated.score).await?
            } else {
                update_region_best(&db, user_id, &row.pool, updated.score).await?
            };
        }
    }

    Ok(Json(RoundGuessResponse {
        result,
        total: updated.score,
        next: next_name.map(|n| NextRound {
            round: updated.round + 1,
            name: n.name,
        }),
        previous_best,
    }))
}

async fn find_cities(db: &D1Database, first: u32, count: u32) -> worker::Result<Vec<CityRow>> {
    let last = first + count - 1;
    db.prepare("SELECT name, country, lat, lon FROM cities WHERE id BETWEEN ?1 AND ?2 ORDER BY id")
        .bind(&[first.into(), last.into()])?
        .all()
        .await?
        .results()
}

#[worker::send]
async fn daily_start(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    body: Option<Json<DailyStartRequest>>,
) -> std::result::Result<axum::response::Response, AppError> {
    let db = env.d1("DB")?;
    let date = body
        .and_then(|Json(req)| req.date)
        .unwrap_or_else(|| utc_date(0));
    let number = playable_daily_number(&date)?;

    let (player, set_cookie) = match current_player(&db, &headers).await? {
        Some(p) => (p, None),
        None => {
            let (id, cookie) = guest::get_or_create_guest(&headers)?;
            (Player::Guest(id), cookie)
        }
    };
    let player_id = player.key();

    db.prepare("INSERT INTO daily_plays (player_id, date) VALUES (?1, ?2) ON CONFLICT DO NOTHING")
        .bind(&[player_id.as_str().into(), date.as_str().into()])?
        .run()
        .await?;

    let row: PlayHistoryRow = db
        .prepare("SELECT round, score, scores FROM daily_plays WHERE player_id = ?1 AND date = ?2")
        .bind(&[player_id.as_str().into(), date.as_str().into()])?
        .first(None)
        .await?
        .ok_or_else(|| AppError::Internal("daily row missing after insert".to_string()))?;

    let secret = env.secret("DAILY_SECRET")?.to_string();
    let ids = daily_ids(&db, &secret, &date).await?;

    let mut scores: Vec<Option<u32>> = row
        .scores
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().ok())
        .collect();
    if scores.len() != row.round as usize {
        scores = vec![None; row.round as usize];
    }

    let mut history = Vec::with_capacity(scores.len());
    for (i, score) in scores.into_iter().enumerate() {
        let name = find_name(&db, ids[i])
            .await?
            .ok_or_else(|| AppError::Internal("daily name id not found".to_string()))?;
        history.push(PlayedRound {
            name: name.name,
            score,
        });
    }

    let next = if row.round < 5 {
        let name = find_name(&db, ids[row.round as usize])
            .await?
            .ok_or_else(|| AppError::Internal("daily name id not found".to_string()))?;
        Some(NextRound {
            round: row.round + 1,
            name: name.name,
        })
    } else {
        None
    };

    let state = DailyState {
        date,
        number,
        total: row.score,
        history,
        next,
    };

    with_cookie(Json(state), set_cookie)
}

#[worker::send]
async fn daily_replay(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    body: Option<Json<DailyStartRequest>>,
) -> ApiResult<Value> {
    let db = env.d1("DB")?;
    let user_id = require_user(&db, &headers).await?;
    if !load_profile(&db, user_id).await?.is_admin {
        return Err(AppError::Unauthorized(
            "only admins can replay a daily challenge",
        ));
    }
    let date = body
        .and_then(|Json(req)| req.date)
        .unwrap_or_else(|| utc_date(0));
    playable_daily_number(&date)?;

    let reset: Option<PlayRow> = db
        .prepare(
            "UPDATE daily_plays SET round = 0, score = 0, scores = '', replays = replays + 1              WHERE player_id = ?1 AND date = ?2 AND round >= 5 RETURNING round, score",
        )
        .bind(&[format!("u:{user_id}").into(), date.as_str().into()])?
        .first(None)
        .await?;
    if reset.is_none() {
        return Err(AppError::BadRequest("finish this daily challenge first"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[worker::send]
async fn daily_guess(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Json(req): Json<DailyGuessRequest>,
) -> ApiResult<RoundGuessResponse> {
    validate_coords(req.lat, req.lon)?;
    playable_daily_number(&req.date)?;

    let db = env.d1("DB")?;

    let player = current_player(&db, &headers)
        .await?
        .ok_or(AppError::BadRequest("start the daily challenge first"))?;
    let player_id = player.key();

    let row: GuessPlayRow = db
        .prepare("SELECT round, replays FROM daily_plays WHERE player_id = ?1 AND date = ?2")
        .bind(&[player_id.as_str().into(), req.date.as_str().into()])?
        .first(None)
        .await?
        .ok_or(AppError::NotFound("daily challenge not started"))?;
    if row.round >= 5 {
        return Err(AppError::BadRequest("already finished"));
    }

    let secret = env.secret("DAILY_SECRET")?.to_string();
    let ids = daily_ids(&db, &secret, &req.date).await?;
    let country_bonus = req.date.as_str() >= game::COUNTRY_BONUS_START;
    let result = score_guess(
        &db,
        ids[row.round as usize],
        req.lat,
        req.lon,
        country_bonus,
        game::WORLD,
    )
    .await?;

    let updated: PlayRow = db
        .prepare(
            "UPDATE daily_plays SET round = round + 1, score = score + ?1, \
             scores = CASE WHEN scores = '' THEN ?5 ELSE scores || ',' || ?5 END, \
             finished_on = CASE WHEN round + 1 >= 5 THEN COALESCE(finished_on, ?6) ELSE finished_on END \
             WHERE player_id = ?2 AND date = ?3 AND round = ?4 \
             RETURNING round, score",
        )
        .bind(&[
            result.score.into(),
            player_id.as_str().into(),
            req.date.as_str().into(),
            row.round.into(),
            result.score.to_string().into(),
            utc_date(0).into(),
        ])?
        .first(None)
        .await?
        .ok_or(AppError::Conflict("round already answered"))?;

    let mut previous_best = None;
    let next = if updated.round < 5 {
        let name = find_name(&db, ids[updated.round as usize])
            .await?
            .ok_or_else(|| AppError::Internal("daily name id not found".to_string()))?;
        Some(NextRound {
            round: updated.round + 1,
            name: name.name,
        })
    } else if row.replays == 0 {
        match &player {
            Player::User(user_id) => {
                record_finish(&db, &req.date, updated.score, None).await?;
                previous_best = update_best(&db, *user_id, updated.score).await?;
            }
            Player::Guest(_) => {
                let ip = guest::ip_hash(&headers, &secret, &req.date);
                record_finish(&db, &req.date, updated.score, Some(&ip)).await?;
            }
        }
        None
    } else {
        None
    };

    Ok(Json(RoundGuessResponse {
        result,
        total: updated.score,
        next,
        previous_best,
    }))
}

async fn score_guess(
    db: &D1Database,
    name_id: u32,
    lat: f64,
    lon: f64,
    country_bonus: bool,
    region: &str,
) -> std::result::Result<GuessResponse, AppError> {
    let name = find_name(db, name_id)
        .await?
        .ok_or(AppError::NotFound("name not found"))?;
    let cities = find_cities(db, name.first_city, name.city_count).await?;
    let mut counted: Vec<usize> = (0..cities.len())
        .filter(|&i| {
            region == game::WORLD
                || continents::continent_of(&cities[i].country, cities[i].lon) == Some(region)
        })
        .collect();
    if counted.is_empty() {
        counted = (0..cities.len()).collect();
    }
    let areas: Vec<CityAreaRow> = db
        .prepare("SELECT city_id, shape FROM city_areas WHERE city_id BETWEEN ?1 AND ?2")
        .bind(&[
            name.first_city.into(),
            (name.first_city + name.city_count - 1).into(),
        ])?
        .all()
        .await?
        .results()?;

    let (closest, centre_m, limits_m) = counted
        .iter()
        .map(|&i| {
            let city_id = name.first_city + i as u32;
            let centre = game::distance_in_meters(lat, lon, cities[i].lat, cities[i].lon);
            let limits = areas
                .iter()
                .find(|a| a.city_id == city_id)
                .and_then(|a| borders::distance_to_area_m(lat, lon, &a.shape))
                .map(|m| m.min(centre));
            (i, centre, limits)
        })
        .min_by(|a, b| a.2.unwrap_or(a.1).total_cmp(&b.2.unwrap_or(b.1)))
        .ok_or_else(|| AppError::Internal(format!("name {} has no cities", name.id)))?;

    let scored_m = limits_m.unwrap_or(centre_m);
    let mut score = game::score(scored_m, region);
    let city_limits = match limits_m {
        Some(m) if m <= 0.0 => Some("inside"),
        Some(m) if game::score(m, region) == game::score(0.0, region) => Some("near"),
        Some(m) if m < centre_m => Some("measured"),
        _ => None,
    };
    if country_bonus && borders::country_at(lat, lon) == Some(cities[closest].country.as_str()) {
        score = game::with_country_bonus(score);
    }
    Ok(GuessResponse {
        distance_m: centre_m,
        score,
        closest,
        cities,
        city_limits,
    })
}

async fn record_finish(
    db: &D1Database,
    date: &str,
    score: u32,
    ip_hash: Option<&str>,
) -> worker::Result<()> {
    if let Some(ip) = ip_hash {
        let row: Option<CountRow> = db
            .prepare(
                "INSERT INTO guest_ip_counts (date, ip_hash, n) VALUES (?1, ?2, 1) \
                 ON CONFLICT (date, ip_hash) DO UPDATE SET n = n + 1 \
                 RETURNING n",
            )
            .bind(&[date.into(), ip.into()])?
            .first(None)
            .await?;
        if row.map_or(1, |r| r.n) > GUEST_LIMIT_PER_IP {
            return Ok(());
        }
    }

    db.prepare(
        "INSERT INTO daily_hist (date, bucket, count) VALUES (?1, ?2, 1) \
         ON CONFLICT (date, bucket) DO UPDATE SET count = count + 1",
    )
    .bind(&[date.into(), game::bucket_for(score).into()])?
    .run()
    .await?;

    Ok(())
}

#[worker::send]
async fn daily_histogram(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Path(date): Path<String>,
) -> ApiResult<HistogramResponse> {
    if date.len() != 10 {
        return Err(AppError::BadRequest("date must look like 2026-10-15"));
    }
    let db = env.d1("DB")?;

    let rows: Vec<BucketRow> = db
        .prepare("SELECT bucket, count FROM daily_hist WHERE date = ?1")
        .bind(&[date.as_str().into()])?
        .all()
        .await?
        .results()?;

    let mut counts = vec![0u32; game::BUCKETS as usize];
    for r in &rows {
        if let Some(c) = counts.get_mut(r.bucket as usize) {
            *c = r.count;
        }
    }
    let players: u32 = counts.iter().sum();

    let mut your_score = None;
    let mut player_id = None;
    if let Some(player) = current_player(&db, &headers).await? {
        let key = player.key();
        let row: Option<PlayRow> = db
            .prepare("SELECT round, score FROM daily_plays WHERE player_id = ?1 AND date = ?2")
            .bind(&[key.as_str().into(), date.as_str().into()])?
            .first(None)
            .await?;
        your_score = row.filter(|r| r.round >= 5).map(|r| r.score);
        player_id = Some(key);
    }

    let others = players.saturating_sub(1);
    let better_than_percent = match (your_score, &player_id) {
        (Some(score), Some(key)) if others > 0 => {
            let your_bucket = game::bucket_for(score);
            let below_bucket: u32 = counts[..your_bucket as usize].iter().sum();
            let same_bucket: CountRow = db
                .prepare(
                    "SELECT COUNT(*) AS n FROM daily_plays WHERE date = ?1 AND round >= 5 \
                     AND score >= ?2 AND score < ?3 AND player_id != ?4",
                )
                .bind(&[
                    date.as_str().into(),
                    (your_bucket * game::BUCKET_SIZE).into(),
                    score.into(),
                    key.as_str().into(),
                ])?
                .first(None)
                .await?
                .ok_or_else(|| AppError::Internal("count query returned nothing".to_string()))?;
            let beaten_in_bucket = same_bucket
                .n
                .min(counts[your_bucket as usize].saturating_sub(1));
            Some(((below_bucket + beaten_in_bucket) * 100 / others).min(100))
        }
        _ => None,
    };

    Ok(Json(HistogramResponse {
        date,
        bucket_starts: (0..game::BUCKETS).map(|i| i * game::BUCKET_SIZE).collect(),
        your_bucket: your_score.map(game::bucket_for),
        counts,
        players,
        your_score,
        better_than_percent,
    }))
}

async fn adopt_guest_play(
    db: &D1Database,
    headers: &HeaderMap,
    user_id: u32,
) -> worker::Result<()> {
    let Some(guest_id) = guest::read_guest_id(headers) else {
        return Ok(());
    };
    let user_player = format!("u:{user_id}");
    let guest_player = format!("g:{guest_id}");
    let date = utc_date(0);

    let moved: Option<PlayRow> = db
        .prepare(
            "UPDATE daily_plays SET player_id = ?1 \
         WHERE player_id = ?2 AND date = ?3 \
         AND NOT EXISTS (SELECT 1 FROM daily_plays WHERE player_id = ?1 AND date = ?3) \
         RETURNING round, score",
        )
        .bind(&[
            user_player.as_str().into(),
            guest_player.as_str().into(),
            date.as_str().into(),
        ])?
        .first::<PlayRow>(None)
        .await?;

    if let Some(play) = moved {
        if play.round >= 5 {
            update_best(db, user_id, play.score).await?;
        }
    }
    Ok(())
}

#[worker::send]
async fn register(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Json(req): Json<Credentials>,
) -> std::result::Result<axum::response::Response, AppError> {
    let email = req.email.trim().to_lowercase();

    if !util::is_valid_email(&email) {
        return Err(AppError::BadRequest("please enter a valid email address"));
    }
    validate_password(&req.password)?;

    let db = env.d1("DB")?;
    enforce(
        &env,
        &db,
        &ratelimit::REGISTER_PER_IP,
        util::client_ip(&headers),
    )
    .await?;
    let pw_hash = auth::hash_password(&req.password).await?;

    let user: IdRow = db
        .prepare(
            "INSERT INTO users (email, pw_hash) VALUES (?1, ?2) \
             ON CONFLICT (email) DO NOTHING RETURNING id",
        )
        .bind(&[email.as_str().into(), pw_hash.as_str().into()])?
        .first(None)
        .await?
        .ok_or(AppError::Conflict("email already registered"))?;

    adopt_guest_play(&db, &headers, user.id).await?;
    let cookie = auth::create_session(&db, user.id).await?;

    if let Err(e) = send_verification_email(&env, &db, user.id, &email).await {
        console_error!("verification email to new user {} failed: {e}", user.id);
    }

    with_cookie(Json(json!({ "email": email })), Some(cookie))
}

async fn enforce(
    env: &Env,
    db: &D1Database,
    limit: &ratelimit::Limit,
    subject: &str,
) -> std::result::Result<(), AppError> {
    let secret = env.secret("DAILY_SECRET")?.to_string();
    if ratelimit::allow(db, limit, subject, &secret).await? {
        Ok(())
    } else {
        Err(AppError::TooManyRequests(
            "too many attempts, please wait a few minutes and try again",
        ))
    }
}

fn validate_password(password: &str) -> std::result::Result<(), AppError> {
    if !(8..=128).contains(&password.chars().count()) {
        return Err(AppError::BadRequest("password must be 8 to 128 characters"));
    }
    Ok(())
}

async fn send_verification_email(
    env: &Env,
    db: &D1Database,
    user_id: u32,
    to: &str,
) -> worker::Result<bool> {
    let Some(token) = email::create_token(db, user_id, email::VERIFY, email::VERIFY_SECS).await?
    else {
        return Ok(false);
    };
    email::send_verification(env, to, &token).await?;
    Ok(true)
}

#[worker::send]
async fn resend_verification(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
) -> ApiResult<Value> {
    let db = env.d1("DB")?;
    let user_id = require_user(&db, &headers).await?;
    let profile = load_profile(&db, user_id).await?;
    if profile.email_verified {
        return Err(AppError::BadRequest("your email is already confirmed"));
    }
    if !send_verification_email(&env, &db, user_id, &profile.email).await? {
        return Err(AppError::TooManyRequests(
            "an email was just sent, please wait a minute",
        ));
    }
    Ok(Json(json!({ "ok": true })))
}

#[worker::send]
async fn verify_email(
    Extension(env): Extension<Env>,
    Json(req): Json<TokenRequest>,
) -> ApiResult<Value> {
    let db = env.d1("DB")?;
    let user_id = email::consume_token(&db, &req.token, email::VERIFY)
        .await?
        .ok_or(AppError::BadRequest("this link is invalid or has expired"))?;

    db.prepare(
        "UPDATE users SET email_verified_at = COALESCE(email_verified_at, ?1) WHERE id = ?2",
    )
    .bind(&[(util::now_secs() as f64).into(), user_id.into()])?
    .run()
    .await?;
    db.prepare("DELETE FROM email_tokens WHERE user_id = ?1 AND purpose = ?2")
        .bind(&[user_id.into(), email::VERIFY.into()])?
        .run()
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[worker::send]
async fn forgot_password(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Json(req): Json<EmailRequest>,
) -> ApiResult<Value> {
    let address = req.email.trim().to_lowercase();
    if !util::is_valid_email(&address) {
        return Err(AppError::BadRequest("please enter a valid email address"));
    }
    let db = env.d1("DB")?;
    // stops one visitor from sending reset emails to lots of strangers through the site
    enforce(
        &env,
        &db,
        &ratelimit::FORGOT_PER_IP,
        util::client_ip(&headers),
    )
    .await?;

    let user: Option<IdRow> = db
        .prepare("SELECT id FROM users WHERE email = ?1")
        .bind(&[address.as_str().into()])?
        .first(None)
        .await?;
    if let Some(user) = user {
        let sent = match email::create_token(&db, user.id, email::RESET, email::RESET_SECS).await? {
            Some(token) => email::send_password_reset(&env, &address, &token).await,
            None => Ok(()),
        };
        if let Err(e) = sent {
            console_error!("password reset email to user {} failed: {e}", user.id);
        }
    }
    Ok(Json(json!({ "ok": true })))
}

#[worker::send]
async fn reset_password(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Json(req): Json<ResetRequest>,
) -> std::result::Result<axum::response::Response, AppError> {
    validate_password(&req.password)?;
    let db = env.d1("DB")?;
    enforce(
        &env,
        &db,
        &ratelimit::RESET_PER_IP,
        util::client_ip(&headers),
    )
    .await?;
    let user_id = email::consume_token(&db, &req.token, email::RESET)
        .await?
        .ok_or(AppError::BadRequest("this link is invalid or has expired"))?;

    let pw_hash = auth::hash_password(&req.password).await?;
    let user: EmailRow = db
        .prepare(
            "UPDATE users SET pw_hash = ?1, email_verified_at = COALESCE(email_verified_at, ?2) \
             WHERE id = ?3 RETURNING email",
        )
        .bind(&[
            pw_hash.as_str().into(),
            (util::now_secs() as f64).into(),
            user_id.into(),
        ])?
        .first(None)
        .await?
        .ok_or_else(|| AppError::Internal(format!("reset token for missing user {user_id}")))?;

    db.prepare("DELETE FROM sessions WHERE user_id = ?1")
        .bind(&[user_id.into()])?
        .run()
        .await?;
    db.prepare("DELETE FROM email_tokens WHERE user_id = ?1")
        .bind(&[user_id.into()])?
        .run()
        .await?;

    adopt_guest_play(&db, &headers, user_id).await?;
    let cookie = auth::create_session(&db, user_id).await?;
    with_cookie(Json(json!({ "email": user.email })), Some(cookie))
}

#[worker::send]
async fn login(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Json(req): Json<Credentials>,
) -> std::result::Result<axum::response::Response, AppError> {
    const WRONG: AppError = AppError::Unauthorized("wrong email or password");

    let email = req.email.trim().to_lowercase();
    if req.password.chars().count() > 128 {
        return Err(WRONG);
    }
    let db = env.d1("DB")?;
    // per IP against one visitor trying many accounts, per email against many IPs trying one account
    enforce(
        &env,
        &db,
        &ratelimit::LOGIN_PER_IP,
        util::client_ip(&headers),
    )
    .await?;
    enforce(&env, &db, &ratelimit::LOGIN_PER_EMAIL, &email).await?;

    let row: Option<LoginRow> = db
        .prepare("SELECT id, pw_hash FROM users WHERE email = ?1")
        .bind(&[email.as_str().into()])?
        .first(None)
        .await?;

    let Some(user) = row else {
        auth::verify_password(&req.password, auth::DUMMY_HASH).await?;
        return Err(WRONG);
    };

    if user.pw_hash.is_empty() {
        return Err(AppError::Unauthorized(
            "this account signs in with Google, use the Google button",
        ));
    }

    if !auth::verify_password(&req.password, &user.pw_hash).await? {
        return Err(WRONG);
    }

    adopt_guest_play(&db, &headers, user.id).await?;
    let cookie = auth::create_session(&db, user.id).await?;
    with_cookie(Json(json!({ "email": email })), Some(cookie))
}

#[worker::send]
async fn logout(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
) -> std::result::Result<axum::response::Response, AppError> {
    if let Some(token) = util::read_cookie(&headers, "session") {
        let db = env.d1("DB")?;
        db.prepare("DELETE FROM sessions WHERE token_hash = ?1")
            .bind(&[util::sha256_hex(&token).into()])?
            .run()
            .await?;
    }
    with_cookie(Json(json!({ "ok": true })), Some(CLEAR_SESSION.to_string()))
}

async fn google_start(
    Extension(env): Extension<Env>,
    Query(query): Query<OAuthStartQuery>,
) -> axum::response::Response {
    let return_to = oauth::safe_return_path(query.return_to.as_deref());
    if !oauth::is_configured(&env) {
        return redirect(&format!("{return_to}?signin=unavailable"), &[]);
    }
    match oauth::start(&env, &return_to) {
        Ok(login) => redirect(&login.redirect_url, &[login.cookie]),
        Err(e) => {
            console_error!("google sign-in could not start: {e}");
            redirect(&format!("{return_to}?signin=failed"), &[])
        }
    }
}

#[worker::send]
async fn google_callback(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
) -> axum::response::Response {
    let pending = oauth::read_pending(util::read_cookie(&headers, oauth::COOKIE));
    let return_to = pending
        .as_ref()
        .map_or_else(|| "/".to_string(), |p| p.return_to.clone());
    let clear = oauth::clear_cookie();

    match finish_google(&env, &headers, pending, query).await {
        Ok(Some((session, new_account))) => {
            let outcome = if new_account { "new" } else { "ok" };
            redirect(&format!("{return_to}?signin={outcome}"), &[clear, session])
        }
        Ok(None) => redirect(&return_to, &[clear]),
        Err(e) => {
            console_error!("google sign-in failed: {e}");
            redirect(&format!("{return_to}?signin=failed"), &[clear])
        }
    }
}

async fn finish_google(
    env: &Env,
    headers: &HeaderMap,
    pending: Option<oauth::Pending>,
    query: OAuthCallbackQuery,
) -> worker::Result<Option<(String, bool)>> {
    let fail = |msg: &str| worker::Error::RustError(msg.to_string());

    if query.error.is_some() {
        return Ok(None);
    }
    let pending = pending.ok_or_else(|| fail("missing or broken oauth cookie"))?;
    let state = query.state.unwrap_or_default();
    if !auth::constant_time_eq(state.as_bytes(), pending.state.as_bytes()) {
        return Err(fail("state does not match"));
    }
    let code = query
        .code
        .filter(|c| !c.is_empty() && c.len() <= 512)
        .ok_or_else(|| fail("missing code"))?;

    let google = oauth::fetch_google_user(env, &code, &pending.verifier).await?;
    let verified = google.email_verified;
    let email = google
        .email
        .map(|e| e.trim().to_lowercase())
        .filter(|e| verified && util::is_valid_email(e))
        .ok_or_else(|| fail("google account has no verified email"))?;

    let db = env.d1("DB")?;
    let (user_id, new_account) = find_or_create_google_user(&db, &google.sub, &email).await?;
    adopt_guest_play(&db, headers, user_id).await?;
    let session = auth::create_session(&db, user_id).await?;
    Ok(Some((session, new_account)))
}

async fn find_or_create_google_user(
    db: &D1Database,
    subject: &str,
    email: &str,
) -> worker::Result<(u32, bool)> {
    let linked: Option<UserIdRow> = db
        .prepare("SELECT user_id FROM user_identities WHERE provider = ?1 AND subject = ?2")
        .bind(&[oauth::GOOGLE.into(), subject.into()])?
        .first(None)
        .await?;
    if let Some(row) = linked {
        return Ok((row.user_id, false));
    }

    let now = util::now_secs() as f64;
    let existing: Option<ExistingUserRow> = db
        .prepare("SELECT id, email_verified_at IS NOT NULL AS verified FROM users WHERE email = ?1")
        .bind(&[email.into()])?
        .first(None)
        .await?;

    let (user_id, new_account) = match existing {
        Some(user) => {
            if user.verified == 0 {
                db.batch(vec![
                    db.prepare(
                        "UPDATE users SET pw_hash = '', email_verified_at = ?1 WHERE id = ?2",
                    )
                    .bind(&[now.into(), user.id.into()])?,
                    db.prepare("DELETE FROM sessions WHERE user_id = ?1")
                        .bind(&[user.id.into()])?,
                    db.prepare("DELETE FROM email_tokens WHERE user_id = ?1")
                        .bind(&[user.id.into()])?,
                ])
                .await?;
            }
            (user.id, false)
        }
        None => {
            let created: IdRow = db
                .prepare(
                    "INSERT INTO users (email, pw_hash, email_verified_at) VALUES (?1, '', ?2) \
                     ON CONFLICT (email) DO NOTHING RETURNING id",
                )
                .bind(&[email.into(), now.into()])?
                .first(None)
                .await?
                .ok_or_else(|| {
                    worker::Error::RustError("the account appeared mid sign-in".to_string())
                })?;
            (created.id, true)
        }
    };

    db.prepare(
        "INSERT INTO user_identities (provider, subject, user_id, created_at) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
    )
    .bind(&[
        oauth::GOOGLE.into(),
        subject.into(),
        user_id.into(),
        now.into(),
    ])?
    .run()
    .await?;
    Ok((user_id, new_account))
}

fn redirect(location: &str, cookies: &[String]) -> axum::response::Response {
    let mut res = StatusCode::FOUND.into_response();
    let headers = res.headers_mut();
    if let Ok(value) = HeaderValue::from_str(location) {
        headers.insert(LOCATION, value);
    }
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(cookie) {
            headers.append(SET_COOKIE, value);
        }
    }
    res
}

#[worker::send]
async fn delete_account(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Json(req): Json<DeleteRequest>,
) -> std::result::Result<axum::response::Response, AppError> {
    const WRONG: AppError = AppError::Unauthorized("wrong password");

    let db = env.d1("DB")?;
    let user_id = require_user(&db, &headers).await?;
    let password = req.password.unwrap_or_default();
    if password.chars().count() > 128 {
        return Err(WRONG);
    }
    enforce(&env, &db, &ratelimit::DELETE_PER_USER, &user_id.to_string()).await?;

    let user: LoginRow = db
        .prepare("SELECT id, pw_hash FROM users WHERE id = ?1")
        .bind(&[user_id.into()])?
        .first(None)
        .await?
        .ok_or_else(|| AppError::Internal(format!("session for missing user {user_id}")))?;
    if !user.pw_hash.is_empty() && !auth::verify_password(&password, &user.pw_hash).await? {
        return Err(WRONG);
    }

    let player_id = format!("u:{user_id}");
    db.batch(vec![
        db.prepare("DELETE FROM daily_plays WHERE player_id = ?1")
            .bind(&[player_id.as_str().into()])?,
        db.prepare("DELETE FROM sessions WHERE user_id = ?1")
            .bind(&[user_id.into()])?,
        db.prepare("DELETE FROM email_tokens WHERE user_id = ?1")
            .bind(&[user_id.into()])?,
        db.prepare("DELETE FROM user_identities WHERE user_id = ?1")
            .bind(&[user_id.into()])?,
        db.prepare("DELETE FROM user_region_bests WHERE user_id = ?1")
            .bind(&[user_id.into()])?,
        db.prepare("DELETE FROM users WHERE id = ?1")
            .bind(&[user_id.into()])?,
    ])
    .await?;

    with_cookie(Json(json!({ "ok": true })), Some(CLEAR_SESSION.to_string()))
}

#[worker::send]
async fn me(Extension(env): Extension<Env>, headers: HeaderMap) -> ApiResult<Profile> {
    let db = env.d1("DB")?;
    let user_id = require_user(&db, &headers).await?;
    Ok(Json(load_profile(&db, user_id).await?))
}

async fn current_player(db: &D1Database, headers: &HeaderMap) -> worker::Result<Option<Player>> {
    if let Some(user_id) = auth::current_user_id(db, headers).await? {
        return Ok(Some(Player::User(user_id)));
    }
    Ok(guest::read_guest_id(headers).map(Player::Guest))
}

async fn update_region_best(
    db: &D1Database,
    user_id: u32,
    region: &str,
    score: u32,
) -> worker::Result<Option<u32>> {
    let before: Option<RegionBest> = db
        .prepare("SELECT region, best FROM user_region_bests WHERE user_id = ?1 AND region = ?2")
        .bind(&[user_id.into(), region.into()])?
        .first(None)
        .await?;
    if before.as_ref().is_some_and(|b| score <= b.best) {
        return Ok(None);
    }
    let raised: Option<RegionBest> = db
        .prepare(
            "INSERT INTO user_region_bests (user_id, region, best) VALUES (?1, ?2, ?3) \
             ON CONFLICT (user_id, region) DO UPDATE SET best = excluded.best \
             WHERE excluded.best > user_region_bests.best RETURNING region, best",
        )
        .bind(&[user_id.into(), region.into(), score.into()])?
        .first(None)
        .await?;
    Ok(raised.map(|_| before.map_or(0, |b| b.best)))
}

async fn update_best(db: &D1Database, user_id: u32, score: u32) -> worker::Result<Option<u32>> {
    #[derive(Deserialize)]
    struct BestRow {
        best_score: u32,
    }
    db.prepare(
        "UPDATE users SET daily_best = CASE WHEN daily_best_date = ?2 THEN MAX(daily_best, ?1) \
         ELSE ?1 END, daily_best_date = ?2 WHERE id = ?3",
    )
    .bind(&[score.into(), utc_date(0).into(), user_id.into()])?
    .run()
    .await?;

    let Some(before) = db
        .prepare("SELECT best_score FROM users WHERE id = ?1")
        .bind(&[user_id.into()])?
        .first::<BestRow>(None)
        .await?
    else {
        return Ok(None);
    };
    if score <= before.best_score {
        return Ok(None);
    }

    let raised: Option<BestRow> = db
        .prepare(
            "UPDATE users SET best_score = ?1 WHERE id = ?2 AND best_score < ?1 RETURNING best_score",
        )
        .bind(&[score.into(), user_id.into()])?
        .first(None)
        .await?;
    Ok(raised.map(|_| before.best_score))
}

async fn require_user(db: &D1Database, headers: &HeaderMap) -> std::result::Result<u32, AppError> {
    auth::current_user_id(db, headers)
        .await?
        .ok_or(AppError::Unauthorized("not logged in"))
}

async fn load_profile(db: &D1Database, user_id: u32) -> std::result::Result<Profile, AppError> {
    let today = utc_date(0);
    let row: ProfileRow = db
        .prepare(
            "SELECT email, email_verified_at IS NOT NULL AS verified, pw_hash != '' AS has_password, \
             (SELECT GROUP_CONCAT(provider) FROM user_identities WHERE user_id = users.id) AS providers, \
             is_admin, best_score, theme, units, \
             CASE WHEN daily_best_date = ?2 THEN daily_best ELSE 0 END AS daily_best \
             FROM users WHERE id = ?1",
        )
        .bind(&[user_id.into(), today.as_str().into()])?
        .first(None)
        .await?
        .ok_or_else(|| AppError::Internal(format!("session for missing user {user_id}")))?;

    let plays: Vec<StreakRow> = db
        .prepare(
            "SELECT date, finished_on FROM daily_plays \
             WHERE player_id = ?1 AND round >= 5 ORDER BY date DESC LIMIT 2000",
        )
        .bind(&[format!("u:{user_id}").into()])?
        .all()
        .await?
        .results()?;
    let region_bests: Vec<RegionBest> = db
        .prepare("SELECT region, best FROM user_region_bests WHERE user_id = ?1 ORDER BY best DESC, region")
        .bind(&[user_id.into()])?
        .all()
        .await?
        .results()?;
    let on_time: Vec<&str> = plays
        .iter()
        .filter(|p| p.finished_on.as_deref() == Some(p.date.as_str()))
        .map(|p| p.date.as_str())
        .collect();

    Ok(Profile {
        email: row.email,
        daily_best: row.daily_best,
        region_bests,
        streak: game::current_streak(&on_time, &today),
        email_verified: row.verified != 0,
        has_password: row.has_password != 0,
        is_admin: row.is_admin != 0,
        providers: row
            .providers
            .map(|p| p.split(',').map(str::to_string).collect())
            .unwrap_or_default(),
        best_score: row.best_score,
        theme: row.theme,
        units: row.units,
    })
}

#[worker::send]
async fn update_settings(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
    Json(req): Json<SettingsRequest>,
) -> ApiResult<Profile> {
    if !matches!(
        req.theme.as_deref(),
        None | Some("system" | "light" | "dark")
    ) {
        return Err(AppError::BadRequest("theme must be system, light or dark"));
    }
    if !matches!(req.units.as_deref(), None | Some("metric" | "imperial")) {
        return Err(AppError::BadRequest("units must be metric or imperial"));
    }

    let db = env.d1("DB")?;
    let user_id = require_user(&db, &headers).await?;
    let theme = req.theme.as_deref().map_or(JsValue::NULL, JsValue::from);
    let units = req.units.as_deref().map_or(JsValue::NULL, JsValue::from);
    db.prepare(
        "UPDATE users SET theme = COALESCE(?1, theme), units = COALESCE(?2, units) WHERE id = ?3",
    )
    .bind(&[theme, units, user_id.into()])?
    .run()
    .await?;

    Ok(Json(load_profile(&db, user_id).await?))
}

fn random_id(min: u32, max: u32) -> u32 {
    let random_num = js_sys::Math::random();
    let range = max - min + 1;
    min + (random_num * range as f64).floor() as u32
}

fn playable_daily_number(date: &str) -> std::result::Result<u32, AppError> {
    let number = game::daily_number(date).ok_or(AppError::BadRequest(
        "there is no daily challenge for that date",
    ))?;
    if date > utc_date(0).as_str() {
        return Err(AppError::BadRequest("that daily challenge isn't out yet"));
    }
    Ok(number)
}

async fn names_by_id(
    db: &D1Database,
    ids: &[u32],
) -> worker::Result<std::collections::HashMap<u32, String>> {
    let mut names = std::collections::HashMap::new();
    for chunk in ids.chunks(90) {
        let placeholders = (1..=chunk.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let values: Vec<JsValue> = chunk.iter().map(|&id| id.into()).collect();
        let rows: Vec<IdNameRow> = db
            .prepare(format!(
                "SELECT id, name FROM names WHERE id IN ({placeholders})"
            ))
            .bind(&values)?
            .all()
            .await?
            .results()?;
        names.extend(rows.into_iter().map(|r| (r.id, r.name)));
    }
    Ok(names)
}

#[worker::send]
async fn daily_archive(
    Extension(env): Extension<Env>,
    headers: HeaderMap,
) -> ApiResult<ArchiveResponse> {
    let today = utc_date(0);
    let (Some(first), Some(last)) = (
        game::day_from_date(game::ARCHIVE_START),
        game::day_from_date(&today),
    ) else {
        return Err(AppError::Internal("archive dates don't parse".to_string()));
    };

    let db = env.d1("DB")?;
    let mut plays = std::collections::HashMap::new();
    if let Some(player) = current_player(&db, &headers).await? {
        let rows: Vec<ArchivePlayRow> = db
            .prepare(
                "SELECT date, round, score FROM daily_plays WHERE player_id = ?1 AND date >= ?2",
            )
            .bind(&[player.key().as_str().into(), game::ARCHIVE_START.into()])?
            .all()
            .await?
            .results()?;
        plays.extend(rows.into_iter().map(|r| (r.date, (r.round, r.score))));
    }

    let secret = env.secret("DAILY_SECRET")?.to_string();
    let saved: Vec<DailyNamesRow> = db
        .prepare("SELECT date, name_ids FROM daily_names WHERE date >= ?1")
        .bind(&[game::ARCHIVE_START.into()])?
        .all()
        .await?
        .results()?;
    let saved: std::collections::HashMap<String, [u32; 5]> = saved
        .into_iter()
        .filter_map(|r| Some((r.date, game::parse_name_ids(&r.name_ids)?)))
        .collect();
    let mut finished_ids = Vec::new();
    for (date, (round, _)) in &plays {
        if *round >= 5 {
            let ids = match saved.get(date) {
                Some(ids) => *ids,
                None => daily_ids(&db, &secret, date).await?,
            };
            finished_ids.push((date.clone(), ids));
        }
    }
    let all_ids: Vec<u32> = finished_ids.iter().flat_map(|(_, ids)| *ids).collect();
    let names = names_by_id(&db, &all_ids).await?;
    let cities_by_date: std::collections::HashMap<String, Vec<String>> = finished_ids
        .into_iter()
        .map(|(date, ids)| {
            let cities = ids
                .iter()
                .map(|id| names.get(id).cloned().unwrap_or_default())
                .collect();
            (date, cities)
        })
        .collect();

    let days = (first..=last)
        .rev()
        .map(|day| {
            let date = game::date_from_day(day);
            let (round, score) = plays.get(&date).copied().unwrap_or((0, 0));
            ArchiveDay {
                number: (day - first + 1) as u32,
                round,
                score,
                cities: cities_by_date.get(&date).cloned(),
                date,
            }
        })
        .collect();

    Ok(Json(ArchiveResponse { today, days }))
}

fn utc_date(days_ago: u32) -> String {
    let ms = js_sys::Date::now() - days_ago as f64 * 86_400_000.0;
    let iso: String = js_sys::Date::new(&JsValue::from_f64(ms))
        .to_iso_string()
        .into();
    iso[..10].to_string()
}

fn with_cookie(
    body: impl IntoResponse,
    set_cookie: Option<String>,
) -> std::result::Result<axum::response::Response, AppError> {
    let mut res = body.into_response();
    if let Some(cookie) = set_cookie {
        let value = cookie
            .parse()
            .map_err(|_| AppError::Internal("bad cookie value".to_string()))?;
        res.headers_mut().insert(SET_COOKIE, value);
    }
    Ok(res)
}

fn validate_coords(lat: f64, lon: f64) -> std::result::Result<(), AppError> {
    if !(-90.0..=90.0).contains(&lat) {
        return Err(AppError::BadRequest("lat must be between -90 and 90"));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(AppError::BadRequest("lon must be between -180 and 180"));
    }
    Ok(())
}
