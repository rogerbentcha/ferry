'use client'

import { domAnimation, LazyMotion, MotionConfig } from 'motion/react'

import { HeroBackdrop, PageEndBackdrop, PageRails } from './backdrop'
import { Features } from './Features'
import { FinalCta } from './FinalCta'
import { Footer } from './Footer'
import { Hero } from './Hero'
import { Install } from './Install'
import { Nav } from './Nav'
import { OpenSource } from './OpenSource'
import { Showcase } from './Showcase'
import { Sources } from './Sources'
import { Stack } from './Stack'

/**
 * The home page of the site (app/(home)/page.tsx). `.landing` scopes its page-wide styles
 * (app/landing.css): the docs pages keep their own.
 */
export function LandingPage() {
  return (
    // Only the animation features of Motion that `Reveal` needs. A visitor who asks for reduced
    // motion gets the fades with no movement.
    <LazyMotion features={domAnimation} strict>
      <MotionConfig reducedMotion="user">
        <div className="landing relative isolate flex-1 overflow-x-clip bg-background font-sans text-foreground">
          <PageRails />
          <HeroBackdrop />
          <PageEndBackdrop />
          <a
            href="#main"
            className="fixed top-3 left-3 z-50 -translate-y-20 rounded-md bg-foreground px-3 py-2 text-sm text-background focus:translate-y-0"
          >
            Skip to content
          </a>
          <Nav />
          <main id="main">
            <Hero />
            <Showcase />
            <Stack />
            <Features />
            <Sources />
            <Install />
            <OpenSource />
            <FinalCta />
          </main>
          <Footer />
        </div>
      </MotionConfig>
    </LazyMotion>
  )
}
