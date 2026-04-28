import { CTABanner } from './components/sections/CTABanner';
import { FAQ } from './components/sections/FAQ';
import { Features } from './components/sections/Features';
import { Footer } from './components/sections/Footer';
import { Hero } from './components/sections/Hero';
import { NewsSection } from './components/sections/NewsSection';
import { Platforms } from './components/sections/Platforms';
import { Soundwave } from './components/sections/Soundwave';
import { StarSubscription } from './components/sections/StarSubscription';
import { Stats } from './components/sections/Stats';

export function App() {
  return (
    <main>
      <Hero />
      <Stats />
      <Features />
      <Soundwave />
      <NewsSection />
      <StarSubscription />
      <Platforms />
      <FAQ />
      <CTABanner />
      <Footer />
    </main>
  );
}
