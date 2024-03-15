# THSDR
## 1. このプログラムの用途
このプログラムは、TH-D75のIFをパソコン上で復調して、音声にすることを目的としています。TH-D75のコントロールコマンドが公開されていないようなので、TH-D75本体のコントロールはKenwood純正のコントロールプログラムで処理することを想定しています。したがって、IFが12[kHz]であれば、TH-D75以外でも使用できると思われます。
## 2. このプログラムの使用方法
現状、簡単なCUIを付けています。どのようなGUIを付けるか検討中のためです。
動作させるとキー入力状態になり、コマンドを受け付けます。コマンドに問題が無ければ、OKが表示されます。コマンドにエラーがあれば、Invalid commandが表示されます。
現在対応しているコマンドは、下記の通りです。
- AM

  IFフィルタを指定します。「AM 6」と入力すると、帯域幅6[kHz]のIFフィルタになります。任意の値のフィルタができる訳ではなく、入力した数値に近く、実装しているフィルタの帯域になります。「AM 20」のように広い帯域を指定すると、IFフィルタ無しになります。
- AGC
  
  「AGC 1」のように入力すると、入力した値に近い時定数でAGCが動作します。「AGC 0」では、0.5秒が動作します。「AGC -1」のように0未満の値を指定するとAGCがOFFになります。現状、対応している時定数は0.5、1.5、2、3、5秒です。現状、AGCをOFFにした場合は、RFゲインが低下しますので、音量が小さくなります。AGCのアルゴリズムは、改良する可能性が高いです。
- AF
  
  AF段のフィルタを選択します。IFフィルタと同様に、入力した数値に近い帯域幅になります。「AF 6」と入力すれば、20[Hz]～6[kHz]のフィルタになります。20[Hz]以下をカットしています。
- END
  
  プログラムを終了します。
## 3. コンパイル時の注意
RSSI表示を付けたため、--releaseオプションを付けない場合は、デバッグ用のメッセージが出力され、画面表示が乱れます。
