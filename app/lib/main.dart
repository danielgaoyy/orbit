import 'package:flutter/material.dart';

import 'core/fleet_controller.dart';
import 'screens/fleet_home_screen.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  final controller = FleetController()..loadSavedHosts();
  runApp(OrbitMobileApp(controller: controller));
}

class OrbitMobileApp extends StatelessWidget {
  const OrbitMobileApp({super.key, required this.controller});

  final FleetController controller;

  @override
  Widget build(BuildContext context) {
    const obsidianBg = Color(0xFF0B0E14);
    const cardSurface = Color(0xFF131822);
    const orbitCyan = Color(0xFF38BDF8);

    return MaterialApp(
      title: 'Orbit',
      debugShowCheckedModeBanner: false,
      themeMode: ThemeMode.dark,
      darkTheme: ThemeData(
        useMaterial3: true,
        brightness: Brightness.dark,
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
        cardTheme: CardThemeData(
          color: cardSurface,
          elevation: 0,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(14),
            side: const BorderSide(color: Color(0xFF1E293B)),
          ),
        ),
      ),
      home: FleetHomeScreen(controller: controller),
    );
  }
}
