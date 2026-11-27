import orbitSmokeTest from './OrbitSmoke.test';
import orbitStatePr1Test from './OrbitStatePr1.test';
import orbitPr2Test from './OrbitPr2.test';
import orbitPr4Test from './OrbitPr4.test';
import orbitPr5Test from './OrbitPr5.test';

export default function testsuite(): void {
  orbitSmokeTest();
  orbitStatePr1Test();
  orbitPr2Test();
  orbitPr4Test();
  orbitPr5Test();
}
