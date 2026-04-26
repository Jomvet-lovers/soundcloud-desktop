import { ChevronDown } from 'lucide-react';
import { faqItems } from '../../constants';
import { Reveal } from '../ui/Reveal';

export function FAQ() {
  return (
    <section className="px-6 py-20 sm:py-24" id="faq">
      <div className="max-w-3xl mx-auto">
        <Reveal className="text-center mb-16">
          <p className="text-xs uppercase tracking-[0.2em] text-[#ff5500] mb-4 font-medium">FAQ</p>
          <h2
            className="text-3xl sm:text-5xl font-bold tracking-tight"
            style={{ fontFamily: "'Satoshi', sans-serif" }}
          >
            Вопросы и ответы
          </h2>
        </Reveal>

        <div className="flex flex-col gap-3 reveal-stagger">
          {faqItems.map((item) => (
            <Reveal key={item.q}>
              <details className="glass faq-item p-5 cursor-pointer group">
                <summary className="font-medium text-[15px] flex items-center justify-between gap-4 text-white/80">
                  {item.q}
                  <ChevronDown size={18} className="chevron text-white/20 shrink-0" />
                </summary>
                <div className="faq-answer">
                  <p className="text-white/45 text-[14px] mt-4 leading-relaxed pl-0">{item.a}</p>
                </div>
              </details>
            </Reveal>
          ))}
        </div>
      </div>
    </section>
  );
}
