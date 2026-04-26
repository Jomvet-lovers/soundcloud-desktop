import { useDownloadCount } from '../../hooks/useDownloadCount';
import { Reveal } from '../ui/Reveal';

export function Stats() {
  const downloads = useDownloadCount();
  const stats = [
    { value: downloads, label: 'скачиваний' },
    { value: '~15 МБ', label: 'установщик' },
    { value: '~100 МБ', label: 'RAM' },
    { value: '60 FPS', label: 'интерфейс' },
  ];

  return (
    <section className="px-6 py-8">
      <Reveal>
        <div className="max-w-4xl mx-auto glass rounded-[28px] p-2">
          <div className="grid grid-cols-2 sm:grid-cols-4">
            {stats.map((s, i) => (
              <div
                key={s.label}
                className={`text-center py-6 px-4 ${
                  i < stats.length - 1 ? 'sm:border-r border-white/[0.06]' : ''
                } ${i < 2 ? 'border-b sm:border-b-0 border-white/[0.06]' : ''}`}
              >
                <div className="text-2xl sm:text-3xl font-bold gradient-text stat-num mb-1">
                  {s.value}
                </div>
                <div className="text-white/35 text-xs uppercase tracking-widest">{s.label}</div>
              </div>
            ))}
          </div>
        </div>
      </Reveal>
    </section>
  );
}
