import '@fontsource-variable/manrope';
import type { Metadata } from 'next';
import { LandingPage } from '@/components/landing/landing-page';
import { appName } from '@/lib/shared';

const title = 'Ferry: your own Render, on one machine';
const description =
  'Ferry turns any server with Docker into your own Render: git or folder in, zero-downtime deploys, a URL out. Open source, written in Rust.';

// Next.js replaces (does not merge) the openGraph and twitter objects of the root layout.
export const metadata: Metadata = {
  title: { absolute: title },
  description,
  openGraph: { type: 'website', siteName: appName, title, description, images: '/og/docs/image.png' },
  twitter: { card: 'summary_large_image', title, description, images: '/og/docs/image.png' },
};

/** The home page of the site: the Ferry landing page (components/landing/). The docs are under /docs. */
export default function HomePage() {
  return <LandingPage />;
}
