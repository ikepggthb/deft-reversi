import pandas as pd
from scipy import stats
import matplotlib.pyplot as plt
from sklearn.linear_model import LinearRegression

def read_data(file_path):
    return pd.read_csv(file_path, skiprows=3, header=None, names=['n_empties', 'search_level', 'search_depth_for_prob_cut', 'search_score', 'search_score_for_prob_cut'])

def analyze_data(df):
    df['prediction_error'] = df['search_score'] - (df['a'] * df['search_score_for_prob_cut'] + df['b'])
    return df['prediction_error'].std()

def plot_error_distribution(df, a, b, n_empties, search_level, search_depth_for_prob_cut):
    e = df['search_score'] - (a * df['search_score_for_prob_cut'] + b)
    plt.figure(figsize=(10, 6))
    plt.hist(e, bins=20, alpha=0.7, label=f'n_empties={n_empties}, level={search_level}')
    plt.xlabel('Prediction Error (e)')
    plt.ylabel('Frequency')
    plt.title('Distribution of Prediction Error')
    plt.text(10, plt.ylim()[1] * 0.90, f'V = {a:.6f}v + {b:.6f} + e')
    plt.text(10, plt.ylim()[1] * 0.85, f'STD: {e.std():.6f}')
    plt.text(10, plt.ylim()[1] * 0.80, f'search_depth_for_prob_cut = {search_depth_for_prob_cut}')
    plt.legend()
    plt.close()

def linear_regression_analysis(df):
    results = []
    for (n_empties, search_level), group in df.groupby(['n_empties', 'search_level']):
        slope, intercept, r_value, _, _ = stats.linregress(group['search_score_for_prob_cut'], group['search_score'])
        e_std = analyze_data(group.assign(a=slope, b=intercept))

        search_depth_for_prob_cut = group['search_depth_for_prob_cut'].iloc[0]
        print(f"n_empties = {n_empties}, search_level = {search_level}")
        print(f"  search_depth_for_prob_cut: {search_depth_for_prob_cut}")
        print(f"  a (slope): {slope:.6f}")
        print(f"  b (intercept): {intercept:.6f}")
        print(f"  R-squared: {r_value**2:.6f}")
        print(f"  e_std: {e_std:.4f}")
        print("")

        results.append([n_empties, search_level, search_depth_for_prob_cut, slope, intercept, e_std])
    return pd.DataFrame(results, columns=['n_empties', 'search_level', 'search_depth_for_prob_cut', 'a', 'b', 'e_std'])

def train_regression_models(df):
    X = df[['n_empties', 'search_level', 'search_depth_for_prob_cut']]
    models = {}
    for target in ['a', 'b', 'e_std']:
        y = df[target]
        model = LinearRegression().fit(X, y)
        models[target] = model
    return models

def display_regression_equations(models):
    print("Regression equation")
    for target, model in models.items():
        print(f"{target} = {model.intercept_:.6f} + {model.coef_[0]:.6f} * n_empties + {model.coef_[1]:.6f} * search_level + {model.coef_[2]:.6f} * search_depth_for_prob_cut")


data = [read_data(f'data/log/mpc/mpc_lv{i}_serach.txt') for i in range(3, 11)]
df = pd.concat(data, ignore_index=True)
linear_regression_results = linear_regression_analysis(df)
models = train_regression_models(linear_regression_results)
display_regression_equations(models)
