export interface ScUser {
  avatar_url: string;
  city: string;
  country: string;
  description: string;
  discogs_name: string;
  first_name: string;
  followers_count: number;
  followings_count: number;
  full_name: string;
  urn: string;
  kind: string;
  created_at: string;
  last_modified: string;
  last_name: string;
  permalink: string;
  permalink_url: string;
  plan: string;
  playlist_count: number;
  public_favorites_count: number;
  reposts_count: number;
  track_count: number;
  uri: string;
  username: string;
  website: string;
  website_title: string;
}

export interface ScMe extends ScUser {
  comments_count: number;
  likes_count: number;
  locale: string;
  online: boolean;
  private_playlists_count: number;
  private_tracks_count: number;
  primary_email_confirmed: boolean;
  quota: {
    unlimited_upload_quota: boolean;
    upload_seconds_used: number;
    upload_seconds_left: number;
  };
  upload_seconds_left: number;
}

export interface ScTrack {
  access: 'playable' | 'preview' | 'blocked';
  artwork_url: string;
  caption: string;
  commentable: boolean;
  comment_count: number;
  created_at: string;
  description: string;
  download_count: number;
  downloadable: boolean;
  duration: number;
  embeddable_by: string;
  full_duration: number;
  genre: string;
  has_downloads_left: boolean;
  urn: string;
  kind: string;
  label_name: string;
  last_modified: string;
  license: string;
  likes_count: number;
  media: {
    transcodings: ScTranscoding[];
  };
  monetization_model: string;
  permalink: string;
  permalink_url: string;
  playback_count: number;
  policy: string;
  public: boolean;
  publisher_metadata: Record<string, unknown>;
  purchase_title: string;
  purchase_url: string;
  release_date: string;
  reposts_count: number;
  secret_token: string;
  sharing: string;
  state: string;
  station_permalink: string;
  station_urn: string;
  streamable: boolean;
  stream_url: string;
  tag_list: string;
  title: string;
  track_format: string;
  uri: string;
  user: ScUser;
  user_id: number;
  waveform_url: string;
  display_date: string;
}

export interface ScTranscoding {
  url: string;
  preset: string;
  duration: number;
  snipped: boolean;
  format: {
    protocol: string;
    mime_type: string;
  };
  quality: string;
}

export interface ScPlaylist {
  artwork_url: string;
  created_at: string;
  description: string;
  duration: number;
  embeddable_by: string;
  genre: string;
  urn: string;
  kind: string;
  label_name: string;
  last_modified: string;
  license: string;
  likes_count: number;
  managed_by_feeds: boolean;
  permalink: string;
  permalink_url: string;
  public: boolean;
  purchase_title: string;
  purchase_url: string;
  release_date: string;
  reposts_count: number;
  secret_token: string;
  set_type: string;
  sharing: string;
  tag_list: string;
  title: string;
  track_count: number;
  tracks: ScTrack[];
  uri: string;
  user: ScUser;
  user_id: number;
}

export interface ScComment {
  body: string;
  created_at: string;
  urn: string;
  kind: string;
  timestamp: number;
  track_id: number;
  uri: string;
  user: ScUser;
  user_id: number;
}

export interface ScWebProfile {
  created_at: string;
  kind: string;
  urn: string;
  network: string;
  title: string;
  url: string;
  username: string;
}

export interface ScStreams {
  http_mp3_128_url?: string;
  hls_mp3_128_url?: string;
  hls_aac_160_url?: string;
  hls_opus_64_url?: string;
  preview_mp3_128_url?: string;
}

export interface ScPaginatedResponse<T> {
  collection: T[];
  next_href?: string;
}

export interface ScActivity {
  type: string;
  created_at: string;
  origin: ScTrack | ScPlaylist;
}

export interface ScTokenResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  scope: string;
  token_type: string;
}
