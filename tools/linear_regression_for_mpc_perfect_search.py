import pandas as pd
from scipy import stats
import matplotlib.pyplot as plt

def read_data():
    f = open('data/log/mpc/mpc_perfect_serach.txt', 'r', encoding='UTF-8')
    input_data = f.readlines()
    f.close()

    # 文字列を行に分割し、各行をカンマで分割して数値に変換、リストに格納
    data = []
    for i, line in enumerate(input_data):
        if i < 3 :
            continue;
        line = line.rstrip("\n")
        numbers = [int(x) for x in line.split(",")]
        data.append(numbers)
    
    return data

def plot_error_distribution(df, a, b, n_empties, search_depth_for_prob_cut):
    V = df['search_score']
    v = df['search_score_for_prob_cut']
    e = V - (a * v + b)  # 予測誤差eの計算
    e_std = e.std();
    
    plt.figure(figsize=(40, 20))
    plt.hist(e, bins=20, alpha=0.7, label=f'n_empties={n_empties}')
    plt.xlabel('Prediction Error (e)')
    plt.ylabel('Frequency')
    plt.title('Distribution of Prediction Error')

    plt.text(10, plt.ylim()[1] * 0.90, f'V = {a:.6f}v + {b:.6f} + e')
    plt.text(10, plt.ylim()[1] * 0.85, f'STD: {e_std:.6f}')
    plt.text(10, plt.ylim()[1] * 0.80, f'search_depth_for_prob_cut = {search_depth_for_prob_cut}')

    plt.legend()
    # plt.savefig(f'data/log/mpc/mpc_for_ps_error_distribution_{n_empties}empties.png', format='png')
    # plt.show()

class mpc_struct():
    def __init__(self,lv, a, b, std):
        self.lv = lv
        self.a = a;
        self.b = b;
        self.std = std;


def gen_mpc_data_perfect_serach():

    data = read_data()
    columns = ['n_empties', 'search_depth_for_prob_cut', 'search_score', 'search_score_for_prob_cut']

    df = pd.DataFrame(data, columns=columns)

    mpcs = list()

    # n_emptiesごとにグループ化して線形回帰を行う
    for n_empties, group in df.groupby('n_empties'):
        V = group['search_score']
        v = group['search_score_for_prob_cut']
        search_depth_for_prob_cut = group['search_depth_for_prob_cut'].iloc[0]
        
        slope, intercept, r_value, p_value, std_err = stats.linregress(v, V)
        e = group['search_score'] - (slope * group['search_score_for_prob_cut'] + intercept)
        e_std = e.std()
        e_mean = e.mean()
        mpcs.append(mpc_struct(search_depth_for_prob_cut, slope, intercept, e_std))
        
        print(f"n_empties = {n_empties}")
        print(f"  search_depth_for_prob_cut: {search_depth_for_prob_cut}")
        print(f"  a (slope): {slope:.5f}")
        print(f"  b (intercept): {intercept:.5f}")
        print(f"  R-squared: {r_value**2:.3f}")
        print(f"  e_std={e_std:.5f}")
        print(f"    99% : +-{e_std*2.576:.2f}")
        print(f"    98% : +-{e_std*2.326:.2f}")
        print(f"    95% : +-{e_std+1.960:.2f}")
        print(f"    90% : +-{e_std+1.645:.2f}")
        print(f"    80% : +-{e_std+1.282:.2f}")
        print("")

        plot_error_distribution(group, slope, intercept, n_empties, search_depth_for_prob_cut)
    
    for i in mpcs:
        print("    Some(MpcParams {" + f"lv: {i.lv}, a: {i.a}, b: {i.b}, e_std: {i.std}" + " }),")

gen_mpc_data_perfect_serach()
