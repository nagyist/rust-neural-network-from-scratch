use rand::Rng;

use super::{RecurrentLayer, RecurrentNetwork};
use crate::{OutputLayer, Weight, IDENTITY, MEAN_SQUARED_ERROR, SIGMOID};

fn build_test_network(input_size: usize, output_size: usize, state_size: usize) -> RecurrentNetwork {
    let mut init_recurrent_weights =
        |_output_ix: usize, _input_ix: usize| -> Weight { rand::thread_rng().gen_range(0.0, 0.1) };
    let mut init_recurrent_biases = |_output_ix: usize| -> Weight { 0. };
    let recurrent_activation_fn = &IDENTITY;

    let mut init_output_weights =
        |_output_ix: usize, _input_ix: usize| -> Weight { rand::thread_rng().gen_range(0.0, 0.1) };
    let mut init_output_biases = |_output_ix: usize| -> Weight { 0. };
    let output_activation_fn = &IDENTITY;

    RecurrentNetwork {
        recurrent_layer: RecurrentLayer::new(
            output_size,
            input_size,
            &mut init_recurrent_weights,
            &mut init_recurrent_biases,
            recurrent_activation_fn,
            &mut init_output_weights,
            &mut init_output_biases,
            output_activation_fn,
            state_size,
        ),
        output_layer: Box::new(OutputLayer::new(
            &IDENTITY,
            &MEAN_SQUARED_ERROR,
            &mut |_, _| 1.,
            input_size,
            output_size,
        )),
        outputs: Vec::new(),
        recurrent_layer_outputs: Vec::new(),
    }
}

/// This is as simple as it gets.  Optimize the weights of the output tree towards zero for all inputs.
#[test]
fn rnn_sanity_output_zero() {
    let input_size = 1;
    let output_size = 1;
    let state_size = 1;
    let learning_rate = 0.25;
    let mut network = build_test_network(input_size, output_size, state_size);

    let training_sequence = vec![vec![1.], vec![0.5]];
    let expected_outputs = vec![Some(vec![0.0]), Some(vec![0.0])];

    let (initial_total_cost, _output_gradients) =
        network.forward_propagate(&training_sequence, Some(&expected_outputs));
    println!("initial cost before training: {}", initial_total_cost);
    println!("initial outputs before training: {:?}", network.outputs);

    let mut last_iter_cost = initial_total_cost;
    for i in 0..10 {
        let new_cost = network.train_one_sequence(&training_sequence, &expected_outputs, learning_rate);
        println!("[{}] cost: {}", i, new_cost);
        println!("[{}] outputs: {:?}", i, network.outputs);
        last_iter_cost = new_cost;
    }
    assert!(last_iter_cost < 0.0001);
}

/// Output current value in the sequence
#[test]
fn rnn_sanity_output_identity() {
    let input_size = 1;
    let output_size = 1;
    let state_size = 1;
    let learning_rate = 0.05;
    let mut network = build_test_network(input_size, output_size, state_size);

    let training_sequence = vec![vec![1.], vec![0.5], vec![1.], vec![0.5]];
    let expected_outputs = vec![Some(vec![1.]), Some(vec![0.5]), Some(vec![1.]), Some(vec![0.5])];

    let (initial_total_cost, _output_gradients) =
        network.forward_propagate(&training_sequence, Some(&expected_outputs));
    println!("initial cost before training: {}", initial_total_cost);
    println!("initial outputs before training: {:?}", network.outputs);

    let mut last_iter_cost = initial_total_cost;
    for i in 0..300 {
        let new_cost = network.train_one_sequence(&training_sequence, &expected_outputs, learning_rate);
        println!("[{}] cost: {}", i, new_cost);
        println!("[{}] outputs: {:?}", i, network.outputs);
        last_iter_cost = new_cost;
    }
    assert!(last_iter_cost < 0.0001);
}

/// Output previous value in the sequence
#[test]
fn rnn_sanity_output_last_value() {
    let input_size = 1;
    let output_size = 1;
    let state_size = 1;
    let learning_rate = 0.05;
    let mut network = build_test_network(input_size, output_size, state_size);

    fn gen_training_data() -> (Vec<Vec<f32>>, Vec<Option<Vec<f32>>>) {
        let sequence_len = rand::thread_rng().gen_range(2usize, 10usize);
        let mut training_sequence = Vec::with_capacity(sequence_len);
        let mut expected_outputs = Vec::with_capacity(sequence_len);

        for i in 0..sequence_len {
            training_sequence.push(vec![rand::thread_rng().gen_range(-1., 1.)]);
            if i == 0 {
                expected_outputs.push(None);
            } else {
                expected_outputs.push(Some(training_sequence[i - 1].clone()));
            }
        }

        (training_sequence, expected_outputs)
    }

    let (training_sequence, expected_outputs) = gen_training_data();
    let (initial_total_cost, _output_gradients) =
        network.forward_propagate(&training_sequence, Some(&expected_outputs));
    println!("initial cost before training: {}", initial_total_cost);
    println!("initial outputs before training: {:?}", network.outputs);

    let mut cost = initial_total_cost;
    for i in 0..1000 {
        let (training_sequence, expected_outputs) = gen_training_data();
        cost = network.train_one_sequence(&training_sequence, &expected_outputs, learning_rate);
        if cost.is_nan() {
            panic!();
        }
        println!("");
        println!("[{}] cost: {}", i, cost);
        println!("[{}] inputs: {:?}", i, training_sequence.as_slice());
        println!("[{}] outputs: {:?}", i, &network.outputs[..training_sequence.len()]);
        println!(
            "[{}] expected: {:?}",
            i,
            expected_outputs
                .into_iter()
                .map(|o| o.unwrap_or_else(|| vec![-0.]))
                .collect::<Vec<_>>()
                .as_slice()
        );
    }
    assert!(cost < 0.001);

    println!(
        "\nRECURRENT WEIGHTS: {:?}",
        network.recurrent_layer.recurrent_tree.weights
    );
    println!("OUTPUT WEIGHTS: {:?}", network.recurrent_layer.output_tree.weights);
    println!("FINAL STATE: {:?}", network.recurrent_layer.state);
}

/// Output value seen 2 steps ago
#[test]
fn rnn_sanity_output_2_steps_back() {
    let input_size = 1;
    let output_size = 1;
    let state_size = 2;
    let learning_rate = 0.01;
    let mut network = build_test_network(input_size, output_size, state_size);

    fn gen_training_data() -> (Vec<Vec<f32>>, Vec<Option<Vec<f32>>>) {
        let sequence_len = rand::thread_rng().gen_range(2usize, 10usize);
        let mut training_sequence = Vec::with_capacity(sequence_len);
        let mut expected_outputs = Vec::with_capacity(sequence_len);

        for i in 0..sequence_len {
            training_sequence.push(vec![rand::thread_rng().gen_range(-1., 1.)]);
            if i < 2 {
                expected_outputs.push(None);
            } else {
                expected_outputs.push(Some(training_sequence[i - 2].clone()));
            }
        }

        (training_sequence, expected_outputs)
    }

    let (training_sequence, expected_outputs) = gen_training_data();
    let (initial_total_cost, _output_gradients) =
        network.forward_propagate(&training_sequence, Some(&expected_outputs));
    println!("initial cost before training: {}", initial_total_cost);
    println!("initial outputs before training: {:?}", network.outputs);

    let mut last_iter_cost = initial_total_cost;
    for i in 0..1000 {
        let (training_sequence, expected_outputs) = gen_training_data();
        let new_cost = network.train_one_sequence(&training_sequence, &expected_outputs, learning_rate);
        if new_cost.is_nan() {
            panic!();
        }
        println!("\n[{}] cost: {}", i, new_cost);
        println!("[{}] inputs: {:?}", i, training_sequence.as_slice());
        println!("[{}] outputs: {:?}", i, &network.outputs[..training_sequence.len()]);
        println!(
            "[{}] expected: {:?}",
            i,
            expected_outputs
                .into_iter()
                .map(|o| o.unwrap_or_else(|| vec![-0.]))
                .collect::<Vec<_>>()
                .as_slice()
        );
        last_iter_cost = new_cost;
    }

    println!(
        "\nRECURRENT WEIGHTS: {:?}",
        network.recurrent_layer.recurrent_tree.weights
    );
    println!("OUTPUT WEIGHTS: {:?}", network.recurrent_layer.output_tree.weights);
    println!("FINAL STATE: {:?}", network.recurrent_layer.state);

    assert!(last_iter_cost < 0.001);
}

#[test]
fn rnn_memory_conditional() {
    let input_size = 1;
    let output_size = 1;
    let state_size = 4;
    let learning_rate = 0.01;
    let mut network = build_test_network(input_size, output_size, state_size);

    // fn gen_training_data() -> Vec<()
}
