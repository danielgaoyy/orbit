import 'package:flutter/material.dart';

import 'core/fleet_controller.dart';
import 'screens/fleet_home_screen.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  final controller = FleetController()..loadSavedHosts();
  runApp(OrbitMobileApp(controller: controller));
}

const List<String> kOrbitCjkFontFallback = [
  'PingFang SC',
  'SF Pro Text',
  'Hiragino Sans GB',
  'Noto Sans CJK SC',
  'Source Han Sans SC',
  'Microsoft YaHei',
  'sans-serif',
];

class OrbitMobileApp extends StatelessWidget {
  const OrbitMobileApp({super.key, required this.controller});

  final FleetController controller;

  @override
  Widget build(BuildContext context) {
    const canvasBg = Color(0xFF0B0D11);
    const surfaceBg = Color(0xFF151922);
    const orbitSky = Color(0xFF38BDF8);

    final baseDark = ThemeData(
      useMaterial3: true,
      brightness: Brightness.dark,
      fontFamilyFallback: kOrbitCjkFontFallback,
    );

    return MaterialApp(
      title: 'Orbit',
      debugShowCheckedModeBanner: false,
      themeMode: ThemeMode.dark,
      darkTheme: baseDark.copyWith(
        scaffoldBackgroundColor: canvasBg,
        colorScheme: const ColorScheme.dark(
          primary: orbitSky,
          secondary: Color(0xFF34D399),
          surface: surfaceBg,
        ),
        appBarTheme: const AppBarTheme(
          backgroundColor: canvasBg,
          surfaceTintColor: Colors.transparent,
          elevation: 0,
        ),
        dialogTheme: DialogThemeData(
          backgroundColor: surfaceBg,
          surfaceTintColor: Colors.transparent,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(20),
            side: BorderSide(color: Colors.white.withValues(alpha: 0.10)),
          ),
        ),
        bottomSheetTheme: const BottomSheetThemeData(
          backgroundColor: surfaceBg,
          surfaceTintColor: Colors.transparent,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.vertical(top: Radius.circular(24)),
          ),
        ),
        cardTheme: CardThemeData(
          color: surfaceBg,
          elevation: 0,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(16),
            side: BorderSide(color: Colors.white.withValues(alpha: 0.08)),
          ),
        ),
      ),
      home: FleetHomeScreen(controller: controller),
    );
  }
}
