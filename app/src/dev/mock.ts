// Só em desenvolvimento (npm run dev, abrindo ?mock=livre|aberto|preparando|host|assistente):
// simula o núcleo Rust com dados de exemplo para revisar o visual no navegador, sem tocar
// nos dados reais. Nunca entra no build de produção (import dinâmico atrás de DEV).
import type { AppInfo, Check, DiaryEvent, ManifestSummary, MemberView, SettingsView, Stats, Status, WorldPlayer } from "../lib/types";

const now = Date.now();
const iso = (minAgo: number) => new Date(now - minAgo * 60_000).toISOString();

function member(nick: string, extra: Partial<MemberView> = {}): MemberView {
  return { id: nick, nick, role: "member", approved: true, online: false, playing: false, hosting: false, is_me: false, last_seen: iso(60 * 26), path: null, ...extra };
}

function status(scene: string): Status {
  const members = [
    member("mchado", { role: "owner", is_me: true, online: true, last_seen: iso(1) }),
    member("paulo_br", { last_seen: iso(90) }),
    member("Sara_23", { last_seen: iso(60 * 5) }),
    member("lucasblock", {}),
    member("biacraft", { last_seen: iso(60 * 50) }),
  ];
  const head: ManifestSummary = {
    id: "m1", kind: "session", created_at: iso(5), session_id: "s1", host_player: "mchado", duration_secs: 7200,
    players: ["mchado", "paulo_br"], total_size: 1.62 * 1024 ** 3, file_count: 1275, restored_from: null, note: null,
  };
  const base: Status = {
    configured: true, phase: "idle", player_name: "mchado", world_name: "pAuletOr 3", is_owner: true,
    lease: { kind: "free", previous: null }, me_hosting: false, head, head_seq: 41, replica_seq: 41, replicating: false,
    online: [], members, sync: null, client_prep: null, address: "localhost", playing: false, host_client_running: false,
    friends_still_playing: false, recovery_pending: false, failover: null, last_error: null, session_started_at: null,
    next_checkpoint_at: null, last_checkpoint_at: iso(5), coord_ok: true,
    net: { started: true, online: true, id_short: "a1b2", path_to_host: null, rtt_ms: null, proxy_port: 25565, proxy_error: null },
    refreshed_at: iso(0),
  };
  if (scene === "aberto") {
    const lease = {
      format: 1, session_number: 42, session_id: "s2", holder: "x", player: "paulo_br", machine: "PC-PAULO", endpoint: null, app_version: "2.0.4",
      purpose: "hosting" as const, acquired_at: iso(40), heartbeat_at: iso(0), vouched_at: null, handing_over: false, online: ["paulo_br", "Sara_23"],
    };
    members[1] = { ...members[1], online: true, playing: true, hosting: true, path: "direta" };
    members[2] = { ...members[2], online: true, playing: true };
    return { ...base, lease: { kind: "held", lease }, net: { ...base.net, path_to_host: "direta", rtt_ms: 18 } };
  }
  if (scene === "preparando") {
    return {
      ...base, phase: "preparing", replicating: false,
      sync: { phase: "assembling", bytes_done: 1.31 * 1024 ** 3, bytes_total: 1.62 * 1024 ** 3, files_done: 1032, files_total: 1275 },
    };
  }
  if (scene === "host") {
    members[0] = { ...members[0], hosting: true, playing: true };
    members[2] = { ...members[2], online: true, playing: true };
    return {
      ...base, phase: "hosting", me_hosting: true, online: ["mchado", "Sara_23"], host_client_running: true,
      session_started_at: iso(83), next_checkpoint_at: new Date(now + 3 * 60_000).toISOString(),
      lease: { kind: "held", lease: { format: 1, session_number: 43, session_id: "s3", holder: "me", player: "mchado", machine: "PC", endpoint: null, app_version: "2.0.4", purpose: "hosting", acquired_at: iso(83), heartbeat_at: iso(0), vouched_at: null, handing_over: false, online: ["mchado", "Sara_23"] } },
    };
  }
  return { ...base, replicating: scene === "livre" };
}

const stats: Stats = {
  players: {
    mchado: { seconds: 61 * 3600, sessions: 40, last_seen: iso(5), days: Array.from({ length: 31 }, (_, i) => `d${i}`) },
    paulo_br: { seconds: 44 * 3600, sessions: 28, last_seen: iso(90), days: Array.from({ length: 22 }, (_, i) => `d${i}`) },
    Sara_23: { seconds: 29 * 3600, sessions: 19, last_seen: iso(300), days: Array.from({ length: 15 }, (_, i) => `d${i}`) },
    biacraft: { seconds: 9 * 3600, sessions: 6, last_seen: iso(3000), days: ["a", "b", "c", "d"] },
  },
  hosts: { mchado: 24, paulo_br: 14, Sara_23: 5 }, host_seconds: {}, days: [], sessions: 43, daily: {},
  firsts: { nether: ["paulo_br", iso(60 * 24 * 20)], end: null, dragon: null }, record: ["mchado", 5.2 * 3600], world_born: iso(60 * 24 * 70),
};
for (let i = 0; i < 70; i++) {
  if ((i * 7) % 3 === 0 || i % 5 === 0) {
    const d = new Date(now - i * 86400_000);
    const k = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    stats.days.push(k);
    stats.daily[k] = { seconds: { mchado: 3600 * (1 + (i % 4)) }, deaths: { paulo_br: i % 3 }, advancements: [] };
  }
}

const diary: DiaryEvent[] = [
  { at: iso(5), kind: "saved", player: "mchado", text: "O mundo foi salvo na nuvem (1,6 GB)." },
  { at: iso(30), kind: "advancement", player: "Sara_23", text: "Sara_23 conseguiu a conquista Diamantes!" },
  { at: iso(42), kind: "death", player: "paulo_br", text: "paulo_br caiu de um lugar alto." },
  { at: iso(80), kind: "joined", player: "Sara_23", text: "Sara_23 entrou no mundo." },
  { at: iso(95), kind: "opened", player: "mchado", text: "mchado abriu o mundo." },
  { at: iso(60 * 26), kind: "handover", player: "paulo_br", text: "paulo_br passou o host para mchado." },
  { at: iso(60 * 27), kind: "left", player: "biacraft", text: "biacraft saiu do mundo." },
];

const checks: Check[] = [
  { id: "coord", title: "Nuvem do mundo", level: "ok", detail: "Respondendo em 84 ms.", fix: null, fix_label: null },
  { id: "java", title: "Java do servidor", level: "ok", detail: "Java 21 pronto.", fix: null, fix_label: null },
  { id: "replica", title: "Cópia local do mundo", level: "ok", detail: "Em dia com a versão 41.", fix: null, fix_label: null },
  { id: "serversdat", title: 'Entrada "pAuletOr 3" no Minecraft', level: "warn", detail: "Falta ou está diferente.", fix: "fix_serversdat", fix_label: "Corrigir" },
  { id: "disk", title: "Espaço em disco", level: "ok", detail: "312 GB livres em D:.", fix: null, fix_label: null },
];

const players: WorldPlayer[] = [
  { uuid: "83035175-0000-4000-8000-000000000001", uuid_version: 4, last_played: iso(60 * 24 * 40), xp_level: 31, items: 214, dimension: "minecraft:overworld", pos: [120, 64, -340], has_advancements: true, has_stats: true, nick: null, singleplayer_owner: true },
  { uuid: "3f1c0000-0000-3000-8000-000000000002", uuid_version: 3, last_played: iso(90), xp_level: 18, items: 96, dimension: "minecraft:the_nether", pos: [12, 70, 8], has_advancements: true, has_stats: true, nick: "paulo_br", singleplayer_owner: false },
];

const settings: SettingsView = {
  config: {
    player_name: "mchado", is_owner: true, coordinator_url: "https://…workers.dev", world_id: "w1", world_name: "pAuletOr 3", minecraft_dir: "C:\\Users\\voce\\AppData\\Roaming\\.minecraft",
    launcher_path: null, direct_launch: true, ram_mb: null, client_ram_mb: null, checkpoint_minutes: 5, grace_minutes: 3, proxy_port: 25565, auto_failover: true,
    wake_on_join: true, background_replica: true, notify_toasts: true, notify_join_leave: true, eula_accepted: true, setup_complete: true,
  },
  has_admin_token: true, data_dir: "D:\\MachenCraft", build: "pessoal", auto_heap_mb: 6144, total_ram_mb: 32768, direct_launch_available: true, auto_client_heap_mb: 4096,
  journal_categories: [["session", "Sessões", true], ["deaths", "Mortes", true], ["advancements", "Conquistas", true], ["chat", "Chat", false]],
};

export function installMock(scene: string) {
  const info: AppInfo = { version: "2.0.4", configured: scene !== "assistente", has_engine: true, data_dir: "D:\\MachenCraft", is_owner: true, start_minimized: false };
  const table: Record<string, unknown> = {
    app_info: info, get_status: status(scene), refresh: status(scene), timeline: [status(scene).head, { ...status(scene).head, id: "m0", kind: "checkpoint", created_at: iso(200) }],
    diary, stats, health: checks, get_settings: settings, world_players: players, invites: [], get_private: { discord_webhook: null },
    get_world_settings: { format: 1, world_name: "pAuletOr 3", motd: "pAuletOr 3", difficulty: "normal", gamemode: "survival", view_distance: 12, simulation_distance: 10, max_players: 10, pvp: true, hardcore: false, seed: "", spawn_protection: 0, allow_flight: false },
    console_history: ["[12:00:01] [Server thread/INFO]: Done (3.2s)!", "[12:04:10] [Server thread/INFO]: Sara_23 joined the game", "[12:09:44] [Server thread/WARN]: Can't keep up!"],
    detect_launchers: [], journal_status: { pending: 0, last_error: null, last_sent_at: null }, update_check: scene === "host" ? null : { version: "2.0.5", notes: null },
    setup_suggest_dir: { suggested_dir: "D:\\MachenCraft", existing_dir: null, minecraft_dir: null, launchers: [], direct_launch: true, free_space: 312 * 1024 ** 3, pending_invite: null, local_world: null },
  };
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback: () => Math.floor(Math.random() * 1e9),
    convertFileSrc: (p: string) => p,
    invoke: async (cmd: string) => {
      if (cmd.startsWith("plugin:")) return cmd.includes("listen") ? 1 : null;
      return cmd in table ? table[cmd] : null;
    },
  };
}
