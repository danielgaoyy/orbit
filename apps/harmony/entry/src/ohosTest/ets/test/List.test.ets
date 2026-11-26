import orbitSmokeTest from './OrbitSmoke.test';
import orbitStatePr1Test from './OrbitStatePr1.test';
import orbitPr2Test from './OrbitPr2.test';

export default function testsuite(): void {
  orbitSmokeTest();
  orbitStatePr1Test();
  orbitPr2Test();
}
