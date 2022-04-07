use crate::{ActivationFunction, DenseLayer, OutputLayer, Weight};

#[cfg(test)]
mod test;

pub struct RecurrentLayer {
    pub state: Vec<Weight>,
    pub recurrent_tree: DenseLayer,
    pub output_tree: DenseLayer,
    pub combined_inputs_scratch: Vec<Weight>,
    pub sequence_inputs: Vec<Vec<Weight>>,
    pub prev_states: Vec<Vec<Weight>>,
    // Computed gradients for each step of the sequence
    pub computed_recurrent_gradients: Vec<Vec<Weight>>,
    pub computed_output_gradients: Vec<Vec<Weight>>,
}

impl RecurrentLayer {
    pub fn new(
        output_count: usize,
        input_count: usize,
        init_recurrent_weights: &mut impl FnMut(usize, usize) -> Weight,
        init_recurrent_biases: &mut impl FnMut(usize) -> Weight,
        recurrent_activation_fn: &'static dyn ActivationFunction,
        init_output_weights: &mut impl FnMut(usize, usize) -> Weight,
        init_output_biases: &mut impl FnMut(usize) -> Weight,
        output_activation_fn: &'static dyn ActivationFunction,
        state_size: usize,
    ) -> Self {
        // State always initialized to all zeros for now
        let state = vec![0.; state_size];

        RecurrentLayer {
            state,
            recurrent_tree: DenseLayer::new(
                state_size,
                input_count + state_size,
                init_recurrent_weights,
                init_recurrent_biases,
                recurrent_activation_fn,
            ),
            output_tree: DenseLayer::new(
                output_count,
                input_count + state_size,
                init_output_weights,
                init_output_biases,
                output_activation_fn,
            ),
            combined_inputs_scratch: vec![0.; input_count + state_size],
            sequence_inputs: Vec::new(),
            prev_states: Vec::new(),
            computed_recurrent_gradients: Vec::new(),
            computed_output_gradients: Vec::new(),
        }
    }

    pub fn reset(&mut self) { self.state.fill(0.); }

    pub fn forward_propagate(&mut self, inputs: &[Weight], index_in_sequence: usize) {
        // Build combined inputs from state, inputs
        self.combined_inputs_scratch[..self.state.len()].copy_from_slice(&self.state);
        self.combined_inputs_scratch[self.state.len()..].copy_from_slice(inputs);

        self.output_tree.forward_propagate(&self.combined_inputs_scratch);
        self.recurrent_tree.forward_propagate(&self.combined_inputs_scratch);

        // Save inputs + previous state for backpropagation
        if let Some(slot) = self.prev_states.get_mut(index_in_sequence) {
            slot.copy_from_slice(&self.state);
        } else {
            self.prev_states.push(self.state.clone());
        }
        if let Some(slot) = self.sequence_inputs.get_mut(index_in_sequence) {
            slot.copy_from_slice(inputs);
        } else {
            self.sequence_inputs.push(inputs.to_vec());
        }

        // Update state
        // TODO: Can eventually avoid copying this buffer
        self.state.copy_from_slice(&self.recurrent_tree.outputs);
    }

    /// Gets the part of the output that is not fed back into the state - the part which is passed on to the next layer.
    pub fn get_outputs(&self) -> &[Weight] { &self.output_tree.outputs }

    pub fn compute_gradients(
        &mut self,
        output_output_weights: &[Vec<Weight>],
        output_gradient_of_output_neurons: &[Vec<Weight>],
        sequence_len: usize,
    ) {
        // Iterate backwards through the sequence, computing gradients for each step.
        //
        // For the last step, the gradient can be computed using the provided weights and gradient of the neurons in the
        // next layer we're connected to.
        //
        // The gradient of the "recurrent" neurons that output the state is set to zero because we don't care about the
        // value of the state at the end of the sequence.

        // These are in reverse order because we're iterating backwards through the sequence
        self.computed_output_gradients = Vec::with_capacity(self.prev_states.len());
        self.computed_recurrent_gradients = Vec::with_capacity(self.prev_states.len());

        // Output -> Output gradients are computed using the provided gradient of the next external layer
        self.output_tree
            .compute_gradients(output_output_weights, output_gradient_of_output_neurons.last().unwrap());
        self.computed_output_gradients
            .push(self.output_tree.neuron_gradients.clone());

        // Recurrent -> Output gradients are computed using the provided gradient of the next external layer
        self.recurrent_tree
            .compute_gradients(output_output_weights, output_gradient_of_output_neurons.last().unwrap());
        self.computed_recurrent_gradients
            .push(self.recurrent_tree.neuron_gradients.clone());
        // Recurrent -> Recurrent gradients are 0 for the last step since we don't care about the state at the end of
        // the sequence.

        let recurrent_recursvely_connected_weights: Vec<_> = self
            .recurrent_tree
            .weights
            .iter()
            .map(|weights| weights[..self.state.len()].to_owned())
            .collect();

        // Continue iterating backwards through the sequence, computing gradients for each step using the gradients of
        // the step after it.
        for i in (0..sequence_len).rev().skip(1) {
            // Output -> Output gradients are computed using the provided gradient of the next external layer
            self.output_tree
                .compute_gradients(output_output_weights, &output_gradient_of_output_neurons[i]);
            self.computed_output_gradients
                .push(self.output_tree.neuron_gradients.clone());

            // Recurrent -> Output gradients are computed using the provided gradient of the next external layer
            self.recurrent_tree
                .compute_gradients(output_output_weights, &output_gradient_of_output_neurons[i]);
            let mut recurrent_to_output_gradients = self.recurrent_tree.neuron_gradients.clone();
            // Recurrent -> Recurrent gradients are computed using the gradients of the step after it and the parts of
            // its own weights that are connected to its own outputs.
            self.recurrent_tree.compute_gradients(
                &recurrent_recursvely_connected_weights,
                self.computed_recurrent_gradients.last().unwrap(),
            );

            // Combine the gradients
            debug_assert_eq!(
                recurrent_to_output_gradients.len(),
                self.recurrent_tree.neuron_gradients.len()
            );
            for i in 0..recurrent_to_output_gradients.len() {
                recurrent_to_output_gradients[i] += self.recurrent_tree.neuron_gradients[i];
            }
            self.computed_recurrent_gradients.push(recurrent_to_output_gradients);
        }

        debug_assert_eq!(
            self.computed_output_gradients.len(),
            self.computed_recurrent_gradients.len()
        );

        self.computed_output_gradients.reverse();
        self.computed_recurrent_gradients.reverse();
    }

    pub fn update_weights(&mut self, learning_rate: Weight, sequence_len: usize) {
        self.combined_inputs_scratch.fill(0.);
        for step_ix in 0..sequence_len {
            // TODO: Don't need to copy inputs into a buffer; can just use the slices directly
            if step_ix == 0 {
                // Internal state is initialized to 0 at the first step of the sequence
            } else {
                // Internal state is initialized to the state from the previous step
                self.combined_inputs_scratch[..self.state.len()].copy_from_slice(&self.prev_states[step_ix]);
            }
            self.combined_inputs_scratch[self.state.len()..].copy_from_slice(&self.sequence_inputs[step_ix]);

            // Maybe we should accumulate the gradients into a scratch buffer instead of adding multiple times?
            for (neuron_ix, &neuron_gradient) in self.computed_output_gradients[step_ix].iter().enumerate() {
                for (weight_ix, weight) in self.output_tree.weights[neuron_ix].iter_mut().enumerate() {
                    *weight += learning_rate * neuron_gradient * self.combined_inputs_scratch[weight_ix];
                }
            }

            for (neuron_ix, &neuron_gradient) in self.computed_recurrent_gradients[step_ix].iter().enumerate() {
                for (weight_ix, weight) in self.recurrent_tree.weights[neuron_ix].iter_mut().enumerate() {
                    *weight += learning_rate * neuron_gradient * self.combined_inputs_scratch[weight_ix];
                }
            }
        }
    }

    pub fn update_biases(&mut self, learning_rate: Weight, sequence_len: usize) {
        for step_ix in 0..sequence_len {
            for neuron_ix in 0..self.output_tree.biases.len() {
                // Each of these biases is added directly to what is fed into our activation function.
                // The impact that it will have on the output of this neuron is equal to
                // whatever the derivative of the activation function is.  We want to update the bias to
                // whatever value minimizes the gradient/error of this neuron.
                self.output_tree.biases[neuron_ix] +=
                    self.computed_output_gradients[step_ix][neuron_ix] * learning_rate;
            }
        }
    }
}

pub struct RecurrentNetwork {
    pub recurrent_layer: RecurrentLayer,
    pub output_layer: Box<OutputLayer>,
    pub recurrent_layer_outputs: Vec<Vec<Weight>>,
    pub outputs: Vec<Vec<Weight>>,
}

impl RecurrentNetwork {
    /// Returns (total_cost, output_gradients)
    pub fn forward_propagate(
        &mut self,
        sequence: &[Vec<Weight>],
        expected_sequence: Option<&[Option<Vec<Weight>>]>,
    ) -> (f32, Vec<Vec<f32>>) {
        // Reset state in recurrent layer to its default value
        self.recurrent_layer.reset();

        let mut output_gradients = Vec::new();
        let mut total_costs = 0.;

        for (step_ix, example) in sequence.iter().enumerate() {
            self.recurrent_layer.forward_propagate(example, step_ix);
            self.output_layer.forward_propagate(self.recurrent_layer.get_outputs());

            match self.outputs.get_mut(step_ix) {
                Some(slot) => slot.copy_from_slice(&self.output_layer.outputs),
                None => self.outputs.push(self.output_layer.outputs.clone()),
            }
            match self.recurrent_layer_outputs.get_mut(step_ix) {
                Some(slot) => slot.copy_from_slice(&self.recurrent_layer.get_outputs()),
                None => self
                    .recurrent_layer_outputs
                    .push(self.recurrent_layer.get_outputs().to_owned()),
            }

            if let Some(expected_sequence) = expected_sequence {
                let gradients = if let Some(expected_output) = &expected_sequence[step_ix] {
                    self.output_layer.compute_costs(expected_output);
                    total_costs += self.output_layer.costs.iter().fold(0., |acc, cost| acc + *cost);
                    self.output_layer.compute_gradients();
                    self.output_layer.neuron_gradients.clone()
                } else {
                    vec![0.; self.output_layer.neuron_gradients.len()]
                };
                output_gradients.push(gradients);
            }
        }

        (total_costs, output_gradients)
    }

    /// Returns the average cost of the output before updating weights.  It would be better to compute again after, but
    /// that would be too expensive
    pub fn train_one_sequence(
        &mut self,
        sequence: &[Vec<Weight>],
        expected_sequence: &[Option<Vec<Weight>>],
        learning_rate: Weight,
    ) -> Weight {
        assert_eq!(sequence.len(), expected_sequence.len());

        // Run the sequence all the way through the network, populating outputs and keeping track of output layer
        // gradients for each step
        let (total_cost, output_gradients) = self.forward_propagate(sequence, Some(expected_sequence));

        // The compute gradients of the recurrent layer for each step of the sequence
        self.recurrent_layer
            .compute_gradients(&self.output_layer.weights, &output_gradients, sequence.len());

        assert_eq!(self.outputs.len(), self.recurrent_layer_outputs.len());
        for i in 0..self.outputs.len() {
            let inputs_to_output_layer = &self.recurrent_layer_outputs[i];
            self.output_layer.update_weights(&inputs_to_output_layer, learning_rate);
        }

        // Update weights + biases of the recurrent layer
        self.recurrent_layer.update_weights(learning_rate, sequence.len());
        self.recurrent_layer.update_biases(learning_rate, sequence.len());

        // That's it, we've successfully "learned"
        (total_cost / self.output_layer.costs.len() as Weight) / sequence.len() as Weight
    }

    pub fn predict(&mut self, sequence: &[Vec<Weight>]) -> &[Vec<Weight>] {
        self.forward_propagate(sequence, None);
        &self.outputs[..sequence.len()]
    }
}
