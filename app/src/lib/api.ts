import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  Category,
  Check,
  DiaryEvent,
  HumanError,
  Invite,
  JoinResult,
  JournalStatus,
  LauncherCandidate,
  LevelInfo,
  ManifestSummary,
  SettingsView,
  SetupSuggestion,
  Stats,
  Status,
  WorldPlayer,
  WorldSettings,
} from "./types";

/** Erro vindo do Rust: sempre tem título humano. */
export function asHuman(e: unknown): HumanError {
  if (e && typeof e === "object" && "title" in e) return e as HumanError;
  return { code: "other", title: "Algo deu errado", detail: String(e), lease: null };
}

export const api = {
  appReady: () => invoke<void>("app_ready"),
  appInfo: () => invoke<AppInfo>("app_info"),
  status: () => invoke<Status>("get_status"),
  refresh: () => invoke<Status>("refresh"),
  play: (openGame: boolean) => invoke<void>("play", { openGame }),
  openGame: () => invoke<void>("open_game"),
  takeTurn: (openGame: boolean) => invoke<void>("take_turn", { openGame }),
  passTurn: () => invoke<void>("pass_turn"),
  stopSession: () => invoke<void>("stop_session"),
  saveNow: () => invoke<void>("save_now"),
  recover: () => invoke<void>("recover"),
  timeline: () => invoke<ManifestSummary[]>("timeline"),
  diary: (limit?: number) => invoke<DiaryEvent[]>("diary", { limit }),
  restore: (manifestId: string) => invoke<void>("restore", { manifestId }),
  stats: () => invoke<Stats>("stats"),
  health: () => invoke<Check[]>("health"),
  runFix: (fix: string) => invoke<string>("run_fix", { fix }),
  diagnostics: () => invoke<string>("diagnostics"),
  settings: () => invoke<SettingsView>("get_settings"),
  saveSettings: (patch: {
    player_name?: string;
    minecraft_dir?: string;
    launcher_path?: string;
    ram_mb?: number;
    checkpoint_minutes?: number;
    grace_minutes?: number;
    notify_toasts?: boolean;
    notify_join_leave?: boolean;
    proxy_port?: number;
    auto_failover?: boolean;
    wake_on_join?: boolean;
    background_replica?: boolean;
    journal?: [Category, boolean][];
  }) => invoke<void>("save_settings", { patch }),
  getPrivate: () => invoke<{ discord_webhook: string | null }>("get_private"),
  setWebhook: (url: string) => invoke<void>("set_webhook", { url }),
  invites: () => invoke<Invite[]>("invites"),
  makeInvite: (hours?: number, uses?: number, approval?: boolean) => invoke<string>("make_invite", { hours, uses, approval }),
  revokeInvite: (id: string) => invoke<void>("revoke_invite", { id }),
  setMember: (id: string, approved: boolean | null, remove: boolean) => invoke<void>("set_member", { id, approved, remove }),
  worldSettings: () => invoke<WorldSettings>("get_world_settings"),
  saveWorldSettings: (settings: WorldSettings) => invoke<void>("save_world_settings", { settings }),
  consoleCmd: (command: string) => invoke<string>("console_cmd", { command }),
  consoleHistory: () => invoke<string[]>("console_history"),
  detectLaunchers: () => invoke<LauncherCandidate[]>("detect_launchers"),
  openLauncher: () => invoke<void>("open_launcher"),
  worldPlayers: () => invoke<WorldPlayer[]>("world_players"),
  migratePlayer: (uuid: string, nick: string) => invoke<unknown>("migrate_player", { uuid, nick }),
  gc: (dryRun: boolean) =>
    invoke<{ manifests_kept: number; manifests_deleted: number; objects_kept: number; objects_deleted: number; bytes_freed: number }>(
      "gc",
      { dryRun },
    ),
  journalStatus: () => invoke<JournalStatus>("journal_status"),
  testWebhook: (url?: string) => invoke<void>("test_webhook", { url }),
  quit: () => invoke<void>("quit_app"),
  hideWindow: () => invoke<void>("hide_window"),

  // primeira execução
  suggestDir: () => invoke<SetupSuggestion>("setup_suggest_dir"),
  setupInit: (args: { data_dir: string; eula: boolean; minecraft_dir?: string; launcher_path?: string }) => invoke<void>("setup_init", { args }),
  invitePreview: (invite: string) => invoke<{ world_id: string; host: string }>("invite_preview", { invite }),
  createWorld: (args: { coordinator_url: string; admin_token: string; name: string; nick: string; folder?: string }) => invoke<void>("create_world", { args }),
  joinWorld: (invite: string, nick: string) => invoke<JoinResult>("join_world", { invite, nick }),
  probeWorldFolder: (folder: string) => invoke<LevelInfo>("probe_world_folder", { folder }),
  ensureServersDat: () => invoke<string>("ensure_serversdat"),
  prepareRuntime: () => invoke<void>("prepare_runtime"),
  finishSetup: () => invoke<void>("finish_setup"),
};
