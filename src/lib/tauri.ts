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
};
