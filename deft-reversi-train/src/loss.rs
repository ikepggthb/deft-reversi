pub fn huber_gradient(prediction: f32, target: f32, delta: f32) -> f32 {
    let error = prediction - target;
    if error.abs() <= delta {
        error
    } else {
        delta * error.signum()
    }
}

pub fn huber_loss(prediction: f32, target: f32, delta: f32) -> f32 {
    let error = (prediction - target).abs();
    if error <= delta {
        0.5 * error * error
    } else {
        delta * (error - 0.5 * delta)
    }
}
