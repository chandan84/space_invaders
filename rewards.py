import math

def reward_function(params):
    # Basic parameters extraction
    on_track = params['all_wheels_on_track']
    dist_center = params['distance_from_center']
    tw = params['track_width']
    spd = params['speed']
    steer = abs(params['steering_angle'])
    prog = params['progress']
    steps_taken = params['steps']
    head = params['heading']
    
    # Early return for off-track
    if not on_track: return 1e-3
    
    # Track analysis
    c_points = params['closest_waypoints']
    wpts = params['waypoints']
    prev_pt, next_pt = wpts[c_points[0]], wpts[c_points[1]]
    
    # Calculate track angle and determine if in curve
    track_dir = (math.degrees(math.atan2(next_pt[1] - prev_pt[1], next_pt[0] - prev_pt[0])) + 360) % 360
    head_diff = abs(track_dir - head)
    head_diff = min(head_diff, 360 - head_diff)
    
    look_ahead = 3
    future_pt = wpts[(c_points[1] + look_ahead) % len(wpts)]
    future_dir = (math.degrees(math.atan2(future_pt[1] - next_pt[1], future_pt[0] - next_pt[0])) + 360) % 360
    curve_sev = abs(future_dir - track_dir)
    curve_sev = min(curve_sev, 360 - curve_sev)
    in_curve = curve_sev > 10
    
    # Opponent awareness
    has_opponents = len(params['objects_location']) > 0
    opponent_factor = 1.0
    
    if has_opponents:
        # Find closest opponent
        obj_locations = params['objects_location']
        obj_speeds = params['objects_speed']
        agent_pos = (params['x'], params['y'])
        
        # Calculate distances to all opponents
        distances = [math.sqrt((obj[0] - agent_pos[0])**2 + (obj[1] - agent_pos[1])**2) for obj in obj_locations]
        
        if distances:
            closest_dist = min(distances)
            closest_idx = distances.index(closest_dist)
            
            # Opponent is close and needs consideration
            if closest_dist < 3.0:
                # Get opponent data
                opponent_speed = obj_speeds[closest_idx]
                
                # Overtaking strategy - adjust line if we're faster
                if spd > opponent_speed + 0.5:
                    # Override optimal distance to facilitate passing
                    overtake_side = 0.7 if params['is_left_of_center'] else -0.3
                    optimal_distance = overtake_side
                    # Encourage higher speed during overtaking
                    opponent_factor = 1.3
                else:
                    # Match opponent speed if we can't pass safely
                    opponent_factor = 0.9
    
    # Racing line calculation
    norm_dist = dist_center / (tw/2)
    opt_dist = 0.3 if in_curve else 0.0
    
    # Check if overtaking should override the optimal distance
    if has_opponents and 'optimal_distance' in locals():
        opt_dist = optimal_distance
    
    # Target speed calculation
    base_speed = max(1.0, 4.0 - (curve_sev/20)) if in_curve else 4.0
    target_spd = base_speed * opponent_factor
    
    # Component rewards
    line_reward = 1.0 if abs(norm_dist - opt_dist) <= 0.1 else max(0.3, 1.0 - (abs(norm_dist - opt_dist) * 2))
    spd_reward = spd / target_spd if spd <= target_spd else max(0.8, 1.0 - ((spd - target_spd) / target_spd))
    steer_reward = 1.0 - (steer / 30.0)
    
    # Racing technique assessment
    technique = 1.0
    if in_curve and params['steering_angle'] * (track_dir - head) < 0 and steer > 5:
        technique = 0.5
    
    # Progress efficiency
    efficiency = prog / steps_taken if steps_taken > 0 else 0.0
    
    # Final reward calculation with weights
    final_reward = (
        (line_reward * 0.35) +       # Racing line
        (spd_reward * 0.35) +        # Speed optimization
        (steer_reward * 0.1) +       # Steering smoothness
        (technique * 0.1) +          # Racing technique
        (efficiency * 0.5)           # Completion efficiency
    )
    
    return float(max(1e-3, final_reward))
