import { memo } from 'react';
import { useNavigate } from 'react-router-dom';
import { type Aura, auraRgba } from '../../lib/aura';
import { Check, Globe, MicVocal } from '../../lib/icons';
import { SocialIcon, socialLabel } from './socials';
import type { ArtistDetail } from './types';

interface ArtistAboutTabProps {
  artist: ArtistDetail;
  aura: Aura;
}

function ArtistAboutTabImpl({ artist, aura }: ArtistAboutTabProps) {
  const navigate = useNavigate();
  return (
    <div className="grid lg:grid-cols-3 gap-6 py-2">
      {/* Bio */}
      <section className="lg:col-span-2 flex flex-col gap-4">
        <h3 className="text-[10px] font-medium text-white/40 flex items-center gap-2">
          <MicVocal size={11} /> {'About'}
        </h3>
        {artist.bio ? (
          <p className="text-[15px] text-white/75 leading-relaxed whitespace-pre-line selectable">
            {artist.bio}
          </p>
        ) : (
          <p className="text-[13px] text-white/30 italic">{'No bio yet'}</p>
        )}
        <div className="flex flex-wrap gap-x-5 gap-y-2">
          {artist.country && (
            <Stat icon={<Globe size={12} />} label={'Country'} value={artist.country} />
          )}
          <Stat
            icon={<Check size={12} className="text-emerald-400" />}
            label={'Confidence'}
            value={`${(artist.confidence * 100).toFixed(0)}%`}
          />
        </div>
      </section>

      {/* Side: SC accounts + extra socials */}
      <div className="flex flex-col gap-6">
        {artist.sc_accounts.length > 0 && (
          <div className="flex flex-col gap-3">
            <h3 className="text-[10px] font-medium text-orange-300/80 flex items-center gap-2">
              <SocialIcon kind="soundcloud" size={11} />
              {'SoundCloud accounts'}
            </h3>
            <div className="flex flex-col gap-2">
              {artist.sc_accounts.map((acc) => (
                <button
                  key={acc.sc_user_id}
                  type="button"
                  onClick={() =>
                    navigate(`/user/${encodeURIComponent(`soundcloud:users:${acc.sc_user_id}`)}`)
                  }
                  className="group flex items-center gap-3 text-left cursor-pointer transition-colors"
                >
                  <span className="w-8 h-8 shrink-0 rounded-full flex items-center justify-center ring-1 ring-white/10 transition-colors group-hover:ring-white/25">
                    <SocialIcon kind="soundcloud" size={14} className="text-orange-300" />
                  </span>
                  <div className="flex-1 min-w-0">
                    <p className="text-[12px] font-semibold text-white/85 truncate transition-colors group-hover:text-white">
                      {acc.role === 'main' ? 'Main' : acc.role === 'demo' ? 'Demo' : acc.role}
                    </p>
                    <p className="text-[10px] text-white/35 tabular-nums truncate">
                      ID {acc.sc_user_id}
                    </p>
                  </div>
                  {acc.verified && <Check size={12} className="text-emerald-400" />}
                </button>
              ))}
            </div>
          </div>
        )}

        {artist.socials.length > 0 && (
          <div className="flex flex-col gap-3">
            <h3 className="text-[10px] font-medium text-white/40">{'Links'}</h3>
            <div className="flex flex-col gap-1">
              {artist.socials.map((s) => (
                <a
                  key={s.url}
                  href={s.url}
                  target="_blank"
                  rel="noreferrer"
                  className="group flex items-center gap-3 px-3 py-2 rounded-xl text-[12px] text-white/70 hover:text-white transition-colors hover:bg-white/[0.04]"
                  style={{ border: '0.5px solid transparent' }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.borderColor = auraRgba(aura, 0.3);
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.borderColor = 'transparent';
                  }}
                >
                  <span className="w-6 h-6 rounded-md flex items-center justify-center text-white/45 group-hover:text-white">
                    <SocialIcon kind={s.kind} size={13} />
                  </span>
                  <span className="flex-1 truncate font-medium">{socialLabel(s.kind)}</span>
                  <span className="text-[9px] text-white/20 font-medium">{s.source}</span>
                </a>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

const Stat = memo(
  ({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) => (
    <span className="inline-flex items-center gap-2 text-[11px] font-semibold">
      <span className="text-white/45">{icon}</span>
      <span className="text-white/40 text-[10px]">{label}</span>
      <span className="text-white/85">{value}</span>
    </span>
  ),
);

export const ArtistAboutTab = memo(ArtistAboutTabImpl);
