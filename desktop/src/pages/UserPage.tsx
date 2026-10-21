import { useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { IdentityHub } from '../components/user/IdentityHub';
import { USER_PAGE_KEYFRAMES } from '../components/user/keyframes';
import { TabDock, type TabId } from '../components/user/TabDock';
import { UserSearchBox } from '../components/user/UserSearchBox';
import {
  UserConnectionsTab,
  UserLikesTab,
  UserPlaylistsTab,
  UserPopularTab,
  UserSearchPlaylistsTab,
  UserSearchTracksTab,
  UserTracksTab,
} from '../components/user/UserTabs';
import { useEditableUserAura, useUserAura } from '../components/user/useUserAura';
import { useUser, useUserSubscription, useUserWebProfiles } from '../lib/hooks';
import { AlertCircle, Loader2 } from '../lib/icons';
import { useRestoredTab } from '../lib/tab-memory';
import { useAuthStore } from '../stores/auth';

/**
 * Какие табы поддерживают inline-поиск по контенту юзера. На followers/
 * following/likes контент принадлежит другим людям / SC owns it — локальный
 * trgm-поиск там не имеет смысла. На popular/tracks/playlists скоуп — наш.
 */
function isSearchableScope(tab: TabId): boolean {
  return tab === 'popular' || tab === 'tracks' || tab === 'playlists';
}

function searchableScopeLabel(tab: TabId): string {
  if (tab === 'playlists') return 'Playlists';
  return 'Tracks';
}

export function UserPage() {
  const { urn } = useParams<{ urn: string }>();
  const navigate = useNavigate();
  const currentUser = useAuthStore((s) => s.user);

  const [activeTab, setActiveTab] = useRestoredTab<TabId>(urn ?? undefined, 'popular');
  // Inline-поиск по контенту юзера. Debounce 350ms — баланс между "не лагает
  // на каждый символ" и "ощущается отзывчиво". Поиск работает только в
  // tracks/popular/playlists скоупах — в followers/following/likes контент
  // принадлежит другим юзерам/SC API, локальный фильтр там бессмысленен.
  const [searchInput, setSearchInput] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  useEffect(() => {
    const handler = setTimeout(() => setDebouncedSearch(searchInput.trim()), 350);
    return () => clearTimeout(handler);
  }, [searchInput]);
  // При смене таба чистим поиск — иначе при переходе followers→tracks инпут
  // покажет старую строку, у которой уже был отдельный контекст. activeTab —
  // именно триггер эффекта, тело его не читает.
  // biome-ignore lint/correctness/useExhaustiveDependencies: activeTab is the trigger
  useEffect(() => {
    setSearchInput('');
    setDebouncedSearch('');
  }, [activeTab]);

  const {
    data: user,
    isLoading: userLoading,
    isError: userError,
    refetch: refetchUser,
  } = useUser(urn);
  const { data: webProfiles } = useUserWebProfiles(urn);

  const isOwnProfile = !!user && currentUser?.urn === user.urn;

  const myStar = false;
  const { data: otherStar = false } = useUserSubscription(!isOwnProfile && urn ? urn : undefined);
  const hasStar = isOwnProfile ? myStar : otherStar;

  const readonly = useUserAura(urn, hasStar && !isOwnProfile);
  const editable = useEditableUserAura(urn, hasStar && isOwnProfile);

  const aura = isOwnProfile ? editable.aura : readonly.aura;
  const customHex = isOwnProfile ? editable.customHex : readonly.customHex;

  const tabs = useMemo(() => {
    if (!user) return [] as const;
    return [
      { id: 'popular' as const, label: 'Popular' },
      { id: 'tracks' as const, label: 'Tracks' },
      { id: 'playlists' as const, label: 'Playlists' },
      { id: 'likes' as const, label: 'Likes' },
      { id: 'followers' as const, label: 'Followers' },
      { id: 'following' as const, label: 'Following' },
    ] as const;
  }, [user]);

  if (userLoading) {
    return (
      <div className="relative w-full min-h-screen flex items-center justify-center">
        <Loader2 size={28} className="text-white/30 animate-spin" />
      </div>
    );
  }

  // A 404/5xx user used to spin forever: `!user` alone kept the loader mounted
  // with no way out.
  if (!user) {
    return (
      <div className="relative w-full min-h-[60vh] flex flex-col items-center justify-center gap-4 px-6 text-center">
        <AlertCircle size={26} className="text-white/25" strokeWidth={1.7} />
        <div className="space-y-1">
          <p className="text-[14px] text-white/70">{'This profile could not be loaded'}</p>
          <p className="text-[12px] text-white/35">
            {userError ? 'The account may have been removed or renamed.' : 'Nothing came back.'}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void refetchUser()}
            className="h-8 px-3.5 rounded-lg text-[12px] font-medium bg-white/[0.06] text-white/80 hover:bg-white/[0.1] transition-colors cursor-pointer"
          >
            {'Try again'}
          </button>
          <button
            type="button"
            onClick={() => navigate(-1)}
            className="h-8 px-3.5 rounded-lg text-[12px] text-white/45 hover:text-white/75 hover:bg-white/[0.06] transition-colors cursor-pointer"
          >
            {'Go back'}
          </button>
        </div>
      </div>
    );
  }

  return (
    <>
      <style>{USER_PAGE_KEYFRAMES}</style>
      <div className="relative w-full min-h-screen">
        <div
          className="relative z-10 w-full max-w-[1480px] mx-auto px-4 md:px-8 pt-6 md:pt-8"
          style={{ isolation: 'isolate' }}
        >
          <IdentityHub
            user={user}
            hasStar={hasStar}
            webProfiles={webProfiles}
            aura={aura}
            isOwnProfile={isOwnProfile}
            customHex={customHex}
            onPickAura={editable.onPickAura}
            onPickCustom={editable.onPickCustom}
          />

          <div className="mt-4 mb-3 flex flex-col md:flex-row md:items-center md:justify-between gap-4">
            <TabDock tabs={tabs} active={activeTab} onChange={setActiveTab} aura={aura} />
            <div className="md:max-w-sm md:w-80 w-full">
              <UserSearchBox
                value={searchInput}
                onChange={setSearchInput}
                scopeLabel={searchableScopeLabel(activeTab)}
                disabled={!isSearchableScope(activeTab)}
              />
            </div>
          </div>

          <div>
            {(() => {
              const searching = !!debouncedSearch && isSearchableScope(activeTab);
              if (searching && (activeTab === 'tracks' || activeTab === 'popular')) {
                return <UserSearchTracksTab urn={urn!} aura={aura} query={debouncedSearch} />;
              }
              if (searching && activeTab === 'playlists') {
                return <UserSearchPlaylistsTab urn={urn!} query={debouncedSearch} />;
              }
              if (activeTab === 'popular') return <UserPopularTab urn={urn!} aura={aura} />;
              if (activeTab === 'tracks') return <UserTracksTab urn={urn!} aura={aura} />;
              if (activeTab === 'playlists') return <UserPlaylistsTab urn={urn!} />;
              if (activeTab === 'likes') return <UserLikesTab key={urn} urn={urn!} aura={aura} />;
              if (activeTab === 'followers')
                return <UserConnectionsTab urn={urn!} mode="followers" />;
              if (activeTab === 'following')
                return <UserConnectionsTab urn={urn!} mode="followings" />;
              return null;
            })()}
          </div>
        </div>
      </div>
    </>
  );
}
