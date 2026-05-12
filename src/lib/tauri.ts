import { invoke } from "@tauri-apps/api/core";

export type UserInfo = {
  login: string;
  name: string | null;
};

export type DeviceCode = {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
};

export type Platform = "windows" | "linux" | "macos";

export type LocalGame = {
  id: string;
  save_path: string;
  display_name?: string | null;
  paused?: boolean;
};

export type CommitInfo = {
  oid: string;
  summary: string;
  author_name: string;
  author_email: string;
  timestamp: number;
};

export type PushOutcome = {
  committed: boolean;
  commit_message: string | null;
  lfs_routed: string[];
};

export type PullOutcome = {
  fast_forwarded: boolean;
  files_synced: number;
};

export type LocalConfig = {
  schema_version: number;
  machine_id: string;
  machine_name: string;
  hostname: string;
  platform: Platform;
  repo_path: string;
  games: LocalGame[];
};

export type GhRepo = {
  name: string;
  full_name: string;
  clone_url: string;
  html_url: string;
  private: boolean;
};

export type InstalledGameDto = {
  steam_appid: number;
  steam_display_name: string;
  ludusavi_name: string | null;
  install_dir: string;
  resolved_save_path: string | null;
  is_auto_addable: boolean;
};

export const api = {
  patConnect: (apiBase: string, token: string): Promise<UserInfo> =>
    invoke("pat_connect", { apiBase, token }),

  oauthClientId: (): Promise<string | null> => invoke("oauth_client_id"),

  oauthStart: (clientId: string): Promise<DeviceCode> =>
    invoke("oauth_start", { clientId }),

  oauthPoll: (clientId: string, device: DeviceCode): Promise<UserInfo> =>
    invoke("oauth_poll", { clientId, device }),

  githubCreateRepo: (name: string): Promise<GhRepo> =>
    invoke("github_create_repo", { name }),

  initRepo: (args: {
    repoUrl: string;
    hostApiBase: string;
    machineName: string;
  }): Promise<LocalConfig> => invoke("init_repo", { args }),

  scanSteam: (): Promise<InstalledGameDto[]> => invoke("scan_steam"),

  addGame: (args: { gameId: string; savePath: string }): Promise<LocalConfig> =>
    invoke("add_game", { args }),

  getLocalConfig: (): Promise<LocalConfig | null> => invoke("get_local_config"),

  openSaveFolder: (gameId: string): Promise<void> =>
    invoke("open_save_folder", { gameId }),

  forcePush: (gameId: string): Promise<PushOutcome> =>
    invoke("force_push", { gameId }),

  forcePull: (gameId: string): Promise<PullOutcome> =>
    invoke("force_pull", { gameId }),

  setGamePaused: (gameId: string, paused: boolean): Promise<LocalConfig> =>
    invoke("set_game_paused", { gameId, paused }),

  renameGame: (args: {
    gameId: string;
    displayName: string | null;
  }): Promise<LocalConfig> => invoke("rename_game", { args }),

  removeGame: (gameId: string): Promise<LocalConfig> =>
    invoke("remove_game", { gameId }),

  listGameCommits: (gameId: string, limit: number): Promise<CommitInfo[]> =>
    invoke("list_game_commits", { gameId, limit }),

  listGameBackups: (gameId: string): Promise<string[]> =>
    invoke("list_game_backups", { gameId }),
};
