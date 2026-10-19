import { type FormEvent, type ReactNode, useRef, useState } from 'react';
import type { UploadMeta } from '../../lib/upload';
import { Row, Segmented, Toggle } from '../settings/primitives';

const LABEL = 'text-[12px] font-medium text-white/55';
const INPUT =
  'w-full rounded-lg border border-white/[0.06] bg-white/[0.04] px-3 py-2 text-[13px] text-white/90 placeholder:text-white/25 outline-none transition-colors focus:border-accent/30';

const SHARING_OPTIONS: ReadonlyArray<{ id: 'public' | 'private'; label: string }> = [
  { id: 'public', label: 'Public' },
  { id: 'private', label: 'Private' },
];

const EMBED_OPTIONS: ReadonlyArray<{ id: 'all' | 'me' | 'none'; label: string }> = [
  { id: 'all', label: 'All' },
  { id: 'me', label: 'Me' },
  { id: 'none', label: 'None' },
];

export interface UploadFormProps {
  value: UploadMeta;
  onChange: (patch: Partial<UploadMeta>) => void;
  /** Blocks submit (e.g. a too-large file that was not confirmed yet). */
  disabled: boolean;
  onSubmit: () => void;
  /** Page-specific extra fields (the artwork picker) above the submit row. */
  children?: ReactNode;
}

/** Controlled metadata form; submits on the native button or Enter. */
export function UploadForm({ value, onChange, disabled, onSubmit, children }: UploadFormProps) {
  const [titleError, setTitleError] = useState(false);
  const titleRef = useRef<HTMLInputElement>(null);

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!value.title.trim()) {
      setTitleError(true);
      titleRef.current?.focus();
      return;
    }
    onSubmit();
  };

  return (
    <form className="flex flex-col gap-5" onSubmit={handleSubmit}>
      <div className="flex flex-col gap-1.5">
        <label className={LABEL} htmlFor="upload-title">
          {'Title'}
        </label>
        <input
          id="upload-title"
          ref={titleRef}
          type="text"
          autoComplete="off"
          autoFocus
          value={value.title}
          onChange={(event) => {
            setTitleError(false);
            onChange({ title: event.target.value });
          }}
          placeholder={'Track title'}
          className={INPUT}
        />
        {titleError && (
          <p role="alert" className="text-[11.5px] text-red-400">
            {'A title is required.'}
          </p>
        )}
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <div className="flex flex-col gap-1.5">
          <label className={LABEL} htmlFor="upload-artist">
            {'Artist'}
          </label>
          <input
            id="upload-artist"
            type="text"
            autoComplete="off"
            value={value.artist ?? ''}
            onChange={(event) => onChange({ artist: event.target.value })}
            placeholder={'Artist name'}
            className={INPUT}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <label className={LABEL} htmlFor="upload-genre">
            {'Genre'}
          </label>
          <input
            id="upload-genre"
            type="text"
            autoComplete="off"
            value={value.genre ?? ''}
            onChange={(event) => onChange({ genre: event.target.value })}
            placeholder={'Genre'}
            className={INPUT}
          />
        </div>
      </div>

      <div className="flex flex-col gap-1.5">
        <label className={LABEL} htmlFor="upload-tags">
          {'Tags'}
        </label>
        <input
          id="upload-tags"
          type="text"
          autoComplete="off"
          value={value.tag_list ?? ''}
          onChange={(event) => onChange({ tag_list: event.target.value })}
          placeholder={'techno, live set'}
          className={INPUT}
        />
        <p className="text-[11.5px] text-white/35">{'Separate tags with commas.'}</p>
      </div>

      <div className="flex flex-col gap-1.5">
        <label className={LABEL} htmlFor="upload-description">
          {'Description'}
        </label>
        <textarea
          id="upload-description"
          rows={4}
          value={value.description ?? ''}
          onChange={(event) => onChange({ description: event.target.value })}
          placeholder={'Tell listeners about this track'}
          className={`${INPUT} resize-none`}
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <label className={LABEL} htmlFor="upload-permalink">
          {'Permalink'}
        </label>
        <input
          id="upload-permalink"
          type="text"
          autoComplete="off"
          value={value.permalink ?? ''}
          onChange={(event) => onChange({ permalink: event.target.value })}
          placeholder={'custom-url-slug'}
          className={INPUT}
        />
        <p className="text-[11.5px] text-white/35">
          {'Custom URL slug. Leave empty to let SoundCloud generate one.'}
        </p>
      </div>

      <div className="flex flex-col divide-y divide-white/[0.05]">
        <Row title={'Sharing'} desc={'Private tracks are only visible to you.'}>
          <div className="w-[176px]">
            <Segmented
              value={value.sharing ?? 'private'}
              options={SHARING_OPTIONS}
              onChange={(sharing) => onChange({ sharing })}
            />
          </div>
        </Row>
        <Row title={'Embedding'} desc={'Who can embed this track.'}>
          <div className="w-[210px]">
            <Segmented
              value={value.embeddable_by ?? 'all'}
              options={EMBED_OPTIONS}
              onChange={(embeddable_by) => onChange({ embeddable_by })}
            />
          </div>
        </Row>
        <Row title={'Downloads'} desc={'Allow listeners to download the original file.'}>
          <Toggle
            checked={value.downloadable ?? true}
            onChange={() => onChange({ downloadable: !(value.downloadable ?? true) })}
          />
        </Row>
        <Row title={'Comments'} desc={'Allow listeners to comment.'}>
          <Toggle
            checked={value.commentable ?? true}
            onChange={() => onChange({ commentable: !(value.commentable ?? true) })}
          />
        </Row>
      </div>

      {children}

      <div className="flex items-center gap-4 border-t border-white/[0.06] pt-5">
        <button
          type="submit"
          disabled={disabled}
          className="cursor-pointer rounded-lg bg-accent px-5 py-2.5 text-[13px] font-semibold text-accent-contrast transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-40"
        >
          {'Upload'}
        </button>
      </div>
    </form>
  );
}
