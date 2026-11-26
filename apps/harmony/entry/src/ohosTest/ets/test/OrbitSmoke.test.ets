import { describe, expect, it } from '@ohos/hypium';

export default function orbitSmokeTest(): void {
  describe('OrbitSmoke', () => {
    it('assertContain', 0, () => {
      expect('ok').assertContain('ok');
    });
  });
}
