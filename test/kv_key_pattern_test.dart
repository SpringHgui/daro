import 'package:daro/widgets/kv_browser_page.dart';
import 'package:flutter_test/flutter_test.dart';

// 键列表过滤词的 glob 口径:裸词按「包含」匹配,写了通配符才按原样发。

void main() {
  group('kvKeyPatternFromInput', () {
    test('裸关键词包成 *词*(包含匹配)', () {
      expect(kvKeyPatternFromInput('user')?.display, '*user*');
      expect(kvKeyPatternFromInput('  user:1  ')?.display, '*user:1*');
    });

    test('空输入不过滤', () {
      expect(kvKeyPatternFromInput(''), isNull);
      expect(kvKeyPatternFromInput('   '), isNull);
    });

    test('已写 glob 的通配符原样透传', () {
      expect(kvKeyPatternFromInput('scan:*')?.display, 'scan:*');
      expect(kvKeyPatternFromInput('*:cache')?.display, '*:cache');
      expect(kvKeyPatternFromInput('a?b')?.display, 'a?b');
    });

    test('转义的字面星号不被二次包裹', () {
      expect(kvKeyPatternFromInput(r'foo\*bar')?.display, r'foo\*bar');
    });
  });
}
