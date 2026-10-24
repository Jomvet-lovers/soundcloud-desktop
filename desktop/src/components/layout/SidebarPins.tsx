import { NavLink } from 'react-router-dom';
import type { SidebarPinnedPlaylist } from '../../stores/settings';
import { playlistMenuHandler } from '../../stores/track-menu';
import { PlaylistCover } from '../music/PlaylistCover';
import { ACTIVE, IconBox, Label, ROW } from './sidebar-primitives';

/** Quick Access: the playlists pinned for the active account. */
export function SidebarPins({
  pinnedPlaylists,
  collapsed,
  registerItem,
}: {
  pinnedPlaylists: SidebarPinnedPlaylist[];
  collapsed: boolean;
  registerItem: (key: string) => (el: HTMLElement | null) => void;
}) {
  if (pinnedPlaylists.length === 0) return null;

  return (
    <div className="px-2 pt-4 space-y-0.5">
      {/* Section header — folds to a hairline divider when collapsed. */}
      <div className="relative h-5 mx-1 mb-0.5">
        <span
          className="absolute inset-x-0 top-1/2 h-px"
          style={{
            background: 'rgba(255,255,255,0.07)',
            opacity: collapsed ? 1 : 0,
            transition: 'opacity 240ms ease',
          }}
        />
        <span
          className="absolute inset-0 flex items-center gap-2 px-2 text-[10px] text-white/25 font-semibold whitespace-nowrap"
          style={{ opacity: collapsed ? 0 : 1, transition: 'opacity 240ms ease' }}
        >
          {'Quick Access'}
        </span>
      </div>

      {pinnedPlaylists.map((playlist) => {
        return (
          <NavLink
            key={playlist.urn}
            ref={registerItem(`pin:${playlist.urn}`)}
            to={`/playlist/${encodeURIComponent(playlist.urn)}`}
            title={collapsed ? playlist.title : undefined}
            onContextMenu={playlistMenuHandler({
              urn: playlist.urn,
              title: playlist.title,
              artworkUrl: playlist.artworkUrl,
            })}
            className={({ isActive }) =>
              `${ROW} ${isActive ? '' : 'text-white/45 hover:text-white/80 hover:bg-white/[0.05]'}`
            }
            style={({ isActive }) => (isActive ? ACTIVE : undefined)}
          >
            <IconBox>
              <span className="flex h-[18px] w-[18px] items-center justify-center overflow-hidden rounded-[5px] ring-1 ring-white/[0.1]">
                <PlaylistCover
                  artworkUrl={playlist.artworkUrl}
                  urn={playlist.urn}
                  size="small"
                  iconSize={17}
                />
              </span>
            </IconBox>
            <Label collapsed={collapsed} className="text-[12.5px] font-medium pr-3">
              {playlist.title}
            </Label>
          </NavLink>
        );
      })}
    </div>
  );
}
