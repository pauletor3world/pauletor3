// Espelho dos tipos serializados pelo núcleo (Rust/serde).

export type Phase =
  | "idle"
  | "preparing"
  | "starting"
  | "hosting"
  | "stopping"
  | "recovering"
  | "maintenance"
  | "waiting_turn";

export interface LockInfo {
  format: number;
  session_id: string;
  player: string;
  machine: string;
  tailscale_ip: string | null;
  address: string | null;
  app_version: string;
  purpose: "hosting" | "maintenance";
  state: "claiming" | "active";
  claimed_at: string;
  started_at: string;
  heartbeat_at: string;
  handing_over: boolean;
  online: string[];
}

export type LockStatus =
  | { kind: "free" }
  | { kind: "held"; lock: LockInfo }
  | { kind: "stale"; lock: LockInfo };

export type ManifestKind = "session" | "checkpoint" | "recovered" | "restore" | "import" | "edit";

export interface ManifestSummary {
  id: string;
  kind: ManifestKind;
  created_at: string;
  session_id: string;
  host_player: string;
  duration_secs: number;
  players: string[];
  total_size: number;
  file_count: number;
  restored_from: string | null;
  note: string | null;
}

export type SyncPhaseName = "scanning" | "uploading" | "downloading" | "verifying" | "assembling" | "publishing";

export interface SyncProgress {
  phase: SyncPhaseName;
  bytes_done: number;
  bytes_total: number;
  files_done: number;
  files_total: number;
}

export interface HumanError {
  code: string;
  title: string;
  detail: string;
  lock: LockInfo | null;
}

export interface Status {
  configured: boolean;
  phase: Phase;
  player_name: string;
  world_name: string;
  lock: LockStatus | null;
  me_hosting: boolean;
  head: ManifestSummary | null;
  online: string[];
  sync: SyncProgress | null;
  address: string | null;
  host_client_running: boolean;
  friends_still_playing: boolean;
  recovery_pending: boolean;
  last_error: HumanError | null;
  session_started_at: string | null;
  next_checkpoint_at: string | null;
  last_checkpoint_at: string | null;
  remote_ok: boolean;
  refreshed_at: string | null;
}

export interface PlayerStats {
  seconds: number;
  sessions: number;
  last_seen: string | null;
  days: string[];
}

export interface DailyStats {
  seconds: Record<string, number>;
  deaths: Record<string, number>;
  advancements: [string, string][];
}

export interface Stats {
  players: Record<string, PlayerStats>;
  hosts: Record<string, number>;
  host_seconds: Record<string, number>;
  days: string[];
  sessions: number;
  daily: Record<string, DailyStats>;
  firsts: {
    nether: [string, string] | null;
    end: [string, string] | null;
    dragon: [string, string] | null;
  };
  record: [string, number] | null;
  world_born: string | null;
}

export interface Check {
  id: string;
  title: string;
  level: "ok" | "warn" | "error";
  detail: string;
  fix: string | null;
  fix_label: string | null;
}

export type AddressMode = "duckdns" | "direct_ip";
export type Category =
  | "session"
  | "checkpoint"
  | "players"
  | "deaths"
  | "advancements"
  | "milestones"
  | "chat"
  | "daily"
  | "problems";

export interface Config {
  player_name: string;
  is_owner: boolean;
  minecraft_dir: string | null;
  launcher_path: string | null;
  ram_mb: number | null;
  checkpoint_minutes: number;
  grace_minutes: number;
  remote_root: string;
  duckdns_domain: string | null;
  address_mode: AddressMode;
  tailscale_key_expires: string | null;
  notify_toasts: boolean;
  notify_join_leave: boolean;
  eula_accepted: boolean;
  setup_complete: boolean;
}

export interface SettingsView {
  config: Config;
  has_webhook: boolean;
  has_duckdns_token: boolean;
  has_tailscale_key: boolean;
  has_remote: boolean;
  data_dir: string;
  auto_heap_mb: number;
  total_ram_mb: number;
  journal_categories: [Category, string, boolean][];
}

export interface WorldSettings {
  format: number;
  world_name: string;
  motd: string;
  difficulty: "peaceful" | "easy" | "normal" | "hard";
  gamemode: "survival" | "creative" | "adventure" | "spectator";
  view_distance: number;
  simulation_distance: number;
  max_players: number;
  pvp: boolean;
  hardcore: boolean;
  seed: string;
  spawn_protection: number;
  allow_flight: boolean;
}

export interface WorldPlayer {
  uuid: string;
  uuid_version: number;
  last_played: string | null;
  xp_level: number;
  items: number;
  dimension: string;
  pos: [number, number, number] | null;
  has_advancements: boolean;
  has_stats: boolean;
  nick: string | null;
  singleplayer_owner: boolean;
}

export interface LauncherCandidate {
  kind: string;
  label: string;
  path: string;
}

export interface TailscaleStatus {
  installed: boolean;
  backend_state: string;
  ipv4: string | null;
  hostname: string | null;
  tailnet: string | null;
  key_expiry: string | null;
}

export interface WorldNet extends TailscaleStatus {
  world_tailnet: string | null;
  on_world: boolean;
}

export interface AppInfo {
  version: string;
  configured: boolean;
  has_engine: boolean;
  data_dir: string | null;
  is_owner: boolean;
  start_minimized: boolean;
}

export interface Notice {
  type: "notice";
  level: "info" | "success" | "warning" | "error";
  title: string;
  body: string;
}

export interface JournalStatus {
  pending: number;
  last_error: string | null;
  last_sent_at: string | null;
}

export interface LevelInfo {
  level_name: string;
  data_version: number;
  version_name: string;
  difficulty: string;
  hardcore: boolean;
  game_type: string;
  seed: number | null;
  singleplayer_uuid: string | null;
}
