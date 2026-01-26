import { ckbadgerApi } from '../fetchers/ckbadger';

export class AddressSampler {
  async sampleTop(count: number): Promise<string[]> {
    const addresses = await ckbadgerApi.getTopAddresses(count);
    return addresses.filter((addr) => addr.address).map((addr) => addr.address!);
  }

  async sampleActive(count: number, days = 7): Promise<string[]> {
    const addresses = await ckbadgerApi.getActiveAddresses({ limit: count, days });
    return addresses.filter((addr) => addr.address).map((addr) => addr.address!);
  }

  async sampleMixed(count: number): Promise<string[]> {
    const topCount = Math.ceil(count / 2);
    const activeCount = count - topCount;

    const [topAddresses, activeAddresses] = await Promise.all([
      this.sampleTop(topCount),
      this.sampleActive(activeCount),
    ]);

    const combined = Array.from(new Set([...topAddresses, ...activeAddresses]));
    return combined.slice(0, count);
  }
}
