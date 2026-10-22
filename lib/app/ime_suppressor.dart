import 'dart:io' show Platform;

import 'package:base_ui_flutter/base_ui_flutter.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// 与 windows/runner/flutter_window.cpp 的 `daro/ime` 通道通信:
/// 密码框获焦时解绑系统输入法(打 bx 就是裸字符,拼音不再被算进圆点数),
/// 失焦时还原。通道按视图 HWND 注册,主窗口与 desktop_multi_window 子窗口各一份。
const _imeChannel = MethodChannel('daro/ime');

/// 把 base-ui 的 [ImeSuppression] 钩子接到原生通道上。
///
/// 只在 Windows 生效:其余平台原生侧没有 `daro/ime` 的 handler,`invokeMethod`
/// 会抛 [MissingPluginException],这里统一吞掉——即「关不上就算了」,退回旧行为,
/// 绝不让密码框因为一个可选的增强而崩。
void installImeSuppression() {
  if (kIsWeb || !Platform.isWindows) return;
  ImeSuppression.handler = (suppress) {
    _imeChannel
        .invokeMethod<void>(suppress ? 'suppress' : 'restore')
        .catchError((Object _) {
      // 无 handler(理论上不会发生:Windows 主/子窗口都注册了)→ 忽略。
    });
  };
}
