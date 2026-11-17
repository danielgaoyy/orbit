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
  'Noto Sans CJK SC',
  'Hiragino Sans GB',
  'Source Han Sans SC',
  'Microsoft YaHei',
  'sans-serif',
];

class OrbitMobileApp extends StatelessWidget {
  const OrbitMobileApp({super.key, required this.controller});

  final FleetController controller;

  @override
  Widget build(BuildContext context) {
    const obsidianBg = Color(0xFF0A0D14);
    const cardSurface = Color(0xFF111622);
    const orbitCyan = Color(0xFF38BDF8);

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
        scaffoldBackgroundColor: obsidianBg,
        colorScheme: const ColorScheme.dark(
          primary: orbitCyan,
          secondary: Color(0xFF818CF8),
          surface: cardSurface,
        ),
        appBarTheme: const AppBarTheme(
          backgroundColor: obsidianBg,
          surfaceTintColor: Colors.transparent,
          elevation: 0,
        ),
        dialogTheme: DialogThemeData(
          backgroundColor: const Color(0xFF111622),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(16),
            side: const BorderSide(color: Color(0xFF1E293B)),
          ),
        ),
        bottomSheetTheme: const BottomSheetThemeData(
          backgroundColor: Color(0xFF111622),
          surfaceTintColor: Colors.transparent,
        ),
        cardTheme: CardThemeData(
          color: cardSurface,
          elevation: 0,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12),
            side: const BorderSide(color: Color(0xFF1E293B)),
          ),
        ),
      ),
      home: FleetHomeScreen(controller: controller),
    );
  }
}
