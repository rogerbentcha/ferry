import { create } from 'zustand'
import { createJSONStorage, persist } from 'zustand/middleware'

import { safeStorage } from '@/lib/storage'

import type { ServicesSort, ServicesView } from './lib'

interface ServicesViewPrefs {
  view: ServicesView
  sort: ServicesSort
  /** The environment the list shows (`null` = all); ignored once it no longer exists. */
  environment: string | null
  setView: (view: ServicesView) => void
  setSort: (sort: ServicesSort) => void
  setEnvironment: (environment: string | null) => void
}

/**
 * Services list preferences (grid/list, sort, environment), persisted per
 * browser. Local to this page until `useUi` grows a `servicesView` field.
 */
export const useServicesViewPrefs = create<ServicesViewPrefs>()(
  persist(
    (set) => ({
      view: 'grid',
      sort: 'name',
      environment: null,
      setView: (view) => set({ view }),
      setSort: (sort) => set({ sort }),
      setEnvironment: (environment) => set({ environment }),
    }),
    {
      name: 'ferry.services-view',
      version: 1,
      storage: createJSONStorage(() => ({
        getItem: (k) => safeStorage.get(k),
        setItem: (k, v) => safeStorage.set(k, v),
        removeItem: (k) => safeStorage.remove(k),
      })),
      partialize: (s) => ({ view: s.view, sort: s.sort, environment: s.environment }),
      merge: (persisted, current) => {
        const p = (persisted ?? {}) as Partial<ServicesViewPrefs>
        return {
          ...current,
          view: p.view === 'list' || p.view === 'grid' ? p.view : current.view,
          sort: p.sort === 'last_deploy' || p.sort === 'name' ? p.sort : current.sort,
          environment: typeof p.environment === 'string' && p.environment !== '' ? p.environment : null,
        }
      },
    },
  ),
)
