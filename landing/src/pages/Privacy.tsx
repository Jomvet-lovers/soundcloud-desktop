export function Privacy() {
  return (
    <div className="min-h-screen bg-[#050507] text-white/90 px-6 py-20">
      <div className="max-w-3xl mx-auto">
        <h1 className="text-4xl font-bold mb-4 gradient-text" style={{ fontFamily: "'Satoshi', sans-serif" }}>
          Политика конфиденциальности
        </h1>
        <p className="text-white/40 mb-12">Обновлено: 1 апреля 2026</p>

        <div className="space-y-8 text-white/70 leading-relaxed">
          <section>
            <h2 className="text-2xl font-semibold text-white/90 mb-4">1. Общие положения</h2>
            <p className="mb-3">
              1.1. Настоящая Политика конфиденциальности регулирует порядок обработки и защиты информации, которую вы передаёте при использовании приложения SoundCloud Desktop.
            </p>
            <p>
              1.2. Используя приложение, вы подтверждаете своё согласие с условиями Политики. Если вы не согласны — прекратите использование приложения.
            </p>
          </section>

          <section>
            <h2 className="text-2xl font-semibold text-white/90 mb-4">2. Сбор информации</h2>
            <p className="mb-3">2.1. Приложение может собирать следующие типы данных:</p>
            <ul className="list-disc list-inside space-y-2 ml-4">
              <li>идентификаторы аккаунта (логин, ID, никнейм);</li>
              <li>техническую информацию (IP-адрес, данные о браузере, устройстве и операционной системе);</li>
              <li>историю взаимодействий с приложением.</li>
            </ul>
            <p className="mt-3">
              2.2. Приложение не требует предоставления паспортных данных, документов, фотографий или другой личной информации, кроме минимально необходимой для работы.
            </p>
          </section>

          <section>
            <h2 className="text-2xl font-semibold text-white/90 mb-4">3. Использование информации</h2>
            <p className="mb-3">3.1. Приложение использует полученную информацию исключительно для:</p>
            <ul className="list-disc list-inside space-y-2 ml-4">
              <li>обеспечения работы функционала;</li>
              <li>связи с пользователем (уведомления и поддержка);</li>
              <li>анализа и улучшения работы приложения.</li>
            </ul>
          </section>

          <section>
            <h2 className="text-2xl font-semibold text-white/90 mb-4">4. Передача информации третьим лицам</h2>
            <p className="mb-3">4.1. Мы не передаём полученные данные третьим лицам, за исключением случаев:</p>
            <ul className="list-disc list-inside space-y-2 ml-4">
              <li>если это требуется по закону;</li>
              <li>если это необходимо для исполнения обязательств перед вами;</li>
              <li>если вы сами дали на это согласие.</li>
            </ul>
          </section>

          <section>
            <h2 className="text-2xl font-semibold text-white/90 mb-4">5. Хранение и защита данных</h2>
            <p className="mb-3">
              5.1. Данные хранятся в течение срока, необходимого для достижения целей обработки.
            </p>
            <p>
              5.2. Мы принимаем разумные меры для защиты данных, но не гарантируем абсолютную безопасность информации при передаче через интернет.
            </p>
          </section>

          <section>
            <h2 className="text-2xl font-semibold text-white/90 mb-4">6. Отказ от ответственности</h2>
            <p className="mb-3">
              6.1. Вы понимаете и соглашаетесь, что передача информации через интернет всегда сопряжена с рисками.
            </p>
            <p>
              6.2. Мы не несём ответственности за утрату, кражу или раскрытие данных, если это произошло по вине третьих лиц или вас самих.
            </p>
          </section>

          <section>
            <h2 className="text-2xl font-semibold text-white/90 mb-4">7. Изменения в Политике</h2>
            <p className="mb-3">
              7.1. Мы вправе изменять условия Политики без предварительного уведомления.
            </p>
            <p>
              7.2. Продолжение использования приложения после внесения изменений означает согласие с новой редакцией Политики.
            </p>
          </section>
        </div>

        <div className="mt-12 pt-8 border-t border-white/10">
          <a href="/" className="text-[#ff5500] hover:text-[#ff7700] transition-colors">
            ← Вернуться на главную
          </a>
        </div>
      </div>
    </div>
  );
}
