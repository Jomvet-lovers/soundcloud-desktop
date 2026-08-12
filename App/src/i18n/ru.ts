/** Русский словарь (дефолтная локаль). Плоские ключи `namespace.name`. */
export const ru = {
  'player.play': 'Играть',
  'player.pause': 'Пауза',
  'player.like': 'Нравится',
  'player.dislike': 'Не нравится',
  'player.abLoop': 'Повтор отрезка A-B',
  'player.playNext': 'Играть следующим',
  'player.addPlaylist': 'Добавить в плейлист',
  'player.shuffle': 'Перемешать',
  'player.prev': 'Предыдущий трек',
  'player.next': 'Следующий трек',
  'player.repeat': 'Повтор',
  'player.eq': 'Эквалайзер',
  'player.lyrics': 'Текст песни',
  'player.queue': 'Очередь',
  'player.mute': 'Без звука',
  'player.tuning': 'Скорость и тональность',
  'player.openTrack': 'Открыть страницу трека',
};

export type I18nKey = keyof typeof ru;
