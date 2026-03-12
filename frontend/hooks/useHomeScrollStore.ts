import { create } from 'zustand';

interface HomeScrollState {
  heroVisible: boolean;
  setHeroVisible: (visible: boolean) => void;
}

export const useHomeScrollStore = create<HomeScrollState>((set) => ({
  heroVisible: true,
  setHeroVisible: (visible) => set({ heroVisible: visible }),
}));
