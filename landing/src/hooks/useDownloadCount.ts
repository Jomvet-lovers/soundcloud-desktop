import { useEffect, useState } from 'react';

export function useDownloadCount() {
  const [count, setCount] = useState<string>('100 000+');
  useEffect(() => {
    fetch(
      'https://raw.githubusercontent.com/zxcloli666/download-history/refs/heads/main/data/zxcloli666_SoundCloud-Desktop.json',
    )
      .then((r) => r.json())
      .then((data: { total: number }[]) => {
        const total = Math.round(data.pop()?.total ?? 0);
        if (total > 0) setCount(`${total.toLocaleString('ru-RU')}+`);
      })
      .catch(() => {});
  }, []);
  return count;
}
