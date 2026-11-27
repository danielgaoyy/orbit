import orbitSmokeTest from './OrbitSmoke.test';
import orbitStatePr1Test from './OrbitStatePr1.test';
import orbitPr4Test from './OrbitPr4.test';

export default function testsuite(): void {
  orbitSmokeTest();
  orbitStatePr1Test();
  orbitPr4Test();
}
