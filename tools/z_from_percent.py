"""
以下は、ChatGPTによって出力されました。

このスクリプトは、指定された確率に対応するZ値を計算します。

#### 使用法:

```
python z_from_percent.py probability
```

- `script_name.py`: このスクリプトのファイル名。
- `probability`: 求めたい確率（パーセンテージ）。

#### 引数:

- `probability`: 求めたい確率（パーセンテージ）を表す数値。0から100の範囲内で指定する必要があります。

#### 出力:

計算された確率に対応するZ値を表示します。

- `{p}%`: 指定された確率（パーセンテージ）。
- `z = {z_value :.5f}`: 計算されたZ値。小数点以下5桁まで表示されます。

#### 例:

```
python script_name.py 95
```

出力:
```
95.0%, z = 1.95996
```

#### 注意事項:

- 引数の確率が0から100の範囲外の場合、エラーメッセージが表示されます。
- このスクリプトはSciPyライブラリを使用しています。SciPyは科学技術計算のためのPythonライブラリです。
"""

import sys
import scipy.stats as stats

def find_z_from_percent(p):
    return stats.norm.ppf(1-((100 - p)/100) * 0.5)

def read_arg():
    try:
        arguments = sys.argv
        p = float(arguments[1])
        if not (0 <= p and p <= 100) :
            print("Err: p [%] の値は、0 ~ 100の間です")
            raise
        return p
    except: 
        print("Err: 引数が正しくありません")
        exit()

def main():
    p = read_arg()
    z_value = find_z_from_percent(p)
    print(f"{p}%, z = {z_value :.5f}")

if __name__ == "__main__":
    main()
