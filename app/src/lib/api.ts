import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  Category,
  Check,
  HumanError,
  JournalStatus,
  LauncherCandidate,
  LevelInfo,
  ManifestSummary,
  SettingsView,
  Stats,
  Status,
  WorldNet,
  WorldPlayer,
  WorldSettings,
  AddressMode,
} from "./types";

/** Erro vindo do Rust: sempre tem título humano. */
export function asHuman(e: unknown): HumanError {
  if (e && typeof e === "object" && "title" in e) return e as HumanError;
  return { code: "other", title: "Algo deu errado", detail: String(e), lock: null };
}

export const api = {
  appReady: () => invoke<void>("app_ready"),
  appInfo: () => invoke<AppInfo>("app_info"),
  status: () => invoke<Status>("get_status"),
  refresh: () => invoke<Status>("refresh"),
  play: (openGame: boolean, takeover?: string[]) => invoke<void>("play", { openGame, takeover }),
  takeTurn: (openGame: boolean) => invoke<void>("take_turn", { openGame }),
  passTurn: () => invoke<void>("pass_turn"),
  stopSession: () => invoke<void>("stop_session"),
  saveNow: () => invoke<void>("save_now"),
  recover: () => invoke<void>("recover"),
  timeline: () => invoke<ManifestSummary[]>("timeline"),
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
    duckdns_domain?: string;
    address_mode?: AddressMode;
    journal?: [Category, boolean][];
  }) => invoke<void>("save_settings", { patch }),
  setSecret: (kind: "discord_webhook" | "duckdns_token" | "tailscale_auth_key", value: string) =>
    invoke<void>("set_secret", { kind, value }),
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

  // assistente
  suggestDir: () =>
    invoke<{
      suggested_dir: string;
      existing_dir: string | null;
      minecraft_dir: string | null;
      launchers: LauncherCandidate[];
      free_space: number | null;
    }>("setup_suggest_dir"),
  setupInit: (args: {
    data_dir: string;
    player_name: string;
    eula: boolean;
    owner: boolean;
    minecraft_dir?: string;
    launcher_path?: string;
  }) => invoke<void>("setup_init", { args }),
  driveAuthorize: (clientId?: string, clientSecret?: string) => invoke<void>("drive_authorize", { clientId, clientSecret }),
  worldProbe: () =>
    invoke<{ has_world: boolean; head: ManifestSummary | null; settings: WorldSettings | null }>("world_probe"),
  importWorld: (folder: string) => invoke<LevelInfo>("import_world", { folder }),
  createWorld: (settings: WorldSettings) => invoke<void>("create_world", { settings }),
  tailscaleStatus: () => invoke<WorldNet>("tailscale_status"),
  tailscaleInstall: () => invoke<void>("tailscale_install"),
  tailscaleUp: (authKey?: string) => invoke<WorldNet>("tailscale_up", { authKey }),
  saveNetwork: (args: {
    duckdns_domain?: string;
    duckdns_token?: string;
    discord_webhook?: string;
    tailscale_auth_key?: string;
    tailscale_key_expires?: string;
  }) => invoke<void>("save_network", { args }),
  testDuckdns: () => invoke<{ address: string; resolved: string[]; expected: string }>("test_duckdns"),
  installFirewall: () => invoke<void>("install_firewall"),
  removeFirewall: () => invoke<void>("remove_firewall"),
  ensureServersDat: () => invoke<string>("ensure_serversdat"),
  prepareRuntime: () => invoke<void>("prepare_runtime"),
  createInvite: (password: string) => invoke<string>("create_invite", { password }),
  acceptInvite: (code: string, password: string) =>
    invoke<{ world_name: string; created_by: string; has_tailscale_key: boolean }>("accept_invite", { code, password }),
  finishSetup: () => invoke<void>("finish_setup"),
};
